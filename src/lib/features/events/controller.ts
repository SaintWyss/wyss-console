import type { EventRow, EventsPage } from '../../contracts/events';
import type {
  ManagedSessionId,
  ManagedTurnId,
} from '../../contracts/identities';
import type { SessionFailure } from '../../contracts/sessions';
import { readEvents, readOlderEvents } from '../../desktop/events-client';
import { SessionCallError } from '../../desktop/sessions-client';
import type { AuthorityObservation } from '../../shell/authority';

export type EventsLink = 'connecting' | 'offline' | 'connected';

export const EVENT_FILTERS = [
  'all',
  'sessions',
  'runtime',
  'turns',
  'approvals',
  'security',
] as const;
export type EventFilter = (typeof EVENT_FILTERS)[number];
/** Only the loaded events of one session, or of one turn of it. */
export interface EventScope {
  session: ManagedSessionId;
  turn: ManagedTurnId | null;
}
/** What lies before the oldest event shown. */
export type EventsStart = 'beginning' | 'more' | 'unavailable';
/** One event, named by the stream it belongs to and its position in it. */
export interface EventKey {
  stream: string;
  sequence: string;
}

export interface EventsState {
  link: EventsLink;
  reading: boolean;
  loadingOlder: boolean;
  /** Why the last read of the newest events failed. */
  readFailure: SessionFailure | null;
  olderFailure: SessionFailure | null;
  /** A window was read at least once since the application started. */
  loaded: boolean;
  /** The stream the events shown belong to. */
  stream: string | null;
  /** Newest first, without a hole. */
  events: EventRow[];
  start: EventsStart;
  /** The newest events that had been loaded were let go to make room for older ones. */
  detached: boolean;
  /** Events newer than the ones shown exist and were not read into the view. */
  newer: boolean;
  /** What is shown was read before the connection was lost, and not since. */
  stale: boolean;
  /** The authority restarted while events were shown. Its stream is the same one. */
  restarted: boolean;
  /** Events of the stream that was shown before this one. Not a continuation of it. */
  previous: { stream: string; events: EventRow[] } | null;
  filter: EventFilter;
  scope: EventScope | null;
  selected: EventKey | null;
}

export interface EventsClient {
  latest(): Promise<EventsPage>;
  older(stream: string, before: string): Promise<EventsPage>;
}
const desktop: EventsClient = { latest: readEvents, older: readOlderEvents };

/** Events kept of the stream that is shown. Past it, the far end is let go. */
export const MAX_EVENTS = 400;
/** Events kept of a stream that was replaced by another. */
export const MAX_PREVIOUS = 100;

function failureOf(error: unknown): SessionFailure {
  return error instanceof SessionCallError
    ? error.failure
    : { code: 'internal', retry: false };
}
function startOf(page: EventsPage): EventsStart {
  if (page.gap) return 'unavailable';
  return page.older === null ? 'beginning' : 'more';
}
/** Whether position `a` lies before `b`. Exact: positions are never numbers. */
function before(a: string | null | undefined, b: string | null): boolean {
  return b !== null && (a == null || BigInt(a) < BigInt(b));
}

/**
 * Presentation state of the Events view. It reads history and changes nothing: it has
 * no action that mutates anything at the authority, and none that performs again what
 * an event describes.
 *
 * It lives for the application's lifetime. It has no watcher and no timer of its own:
 * it is told when the authority's durable head moves by the shell's one follower, and
 * reads only while the view is shown. What it reads is evidence of what happened; what
 * a session, a turn or an approval is now is never taken from it.
 *
 * Positions are the stream's own text. The view hands back the one a page gave it and
 * never makes one up; positions of two streams are never compared or mixed.
 */
export class EventsController {
  private link: EventsLink = 'connecting';
  private head: string | null = null;
  private restarts: number | null = null;
  private visible = false;
  private atTop = true;
  /** The head of the window that was last read. */
  private shownHead: string | null = null;
  private triedHead: string | null | undefined = undefined;
  /** The head at which a read failed. It is not repeated by itself until that changes. */
  private failedHead: string | null | undefined = undefined;
  /** The connection came back, or the authority restarted: read once to learn where
   * history is, whatever the head says. */
  private recheck = false;
  private reading = false;
  private wanted: 'merge' | 'replace' | null = null;
  private loadingOlder = false;
  private readFailure: SessionFailure | null = null;
  private olderFailure: SessionFailure | null = null;
  private loaded = false;
  private stream: string | null = null;
  private events: EventRow[] = [];
  private older: string | null = null;
  private start: EventsStart = 'beginning';
  private detached = false;
  private newer = false;
  private stale = false;
  private restarted = false;
  private previous: { stream: string; events: EventRow[] } | null = null;
  private filter: EventFilter = 'all';
  private scope: EventScope | null = null;
  private selected: EventKey | null = null;
  /** Reads issued so far. An answer to anything but the latest is discarded. */
  private generation = 0;
  /** Likewise for the window before the oldest event: it answers one list of events. */
  private olderEpoch = 0;
  private disposed = false;

  constructor(
    private readonly update: (state: EventsState) => void,
    private readonly client: EventsClient = desktop,
  ) {}

  state(): EventsState {
    return {
      link: this.link,
      reading: this.reading,
      loadingOlder: this.loadingOlder,
      readFailure: this.readFailure,
      olderFailure: this.olderFailure,
      loaded: this.loaded,
      stream: this.stream,
      events: this.events,
      start: this.start,
      detached: this.detached,
      newer: this.newer,
      stale: this.stale,
      restarted: this.restarted,
      previous: this.previous,
      filter: this.filter,
      scope: this.scope,
      selected: this.selected,
    };
  }
  private publish() {
    if (this.disposed) return;
    // A selection is of an event that is still shown.
    const key = this.selected;
    if (key !== null) {
      const shown =
        key.stream === this.stream
          ? this.events
          : key.stream === this.previous?.stream
            ? this.previous.events
            : [];
      if (!shown.some((event) => event.sequence === key.sequence))
        this.selected = null;
    }
    this.update(this.state());
  }

  /** The shell's one authority follower feeds this. No second follower exists. */
  observe(observation: AuthorityObservation) {
    if (this.disposed) return;
    const view = observation.state === 'observed' ? observation.view : null;
    if (view?.connection === 'connected' && view.authority) {
      if (this.link !== 'connected' && this.loaded) this.recheck = true;
      // Another instance on the same socket. Whether its history is the same stream
      // is read, not assumed.
      if (this.restarts !== null && this.restarts !== view.authority.restarts) {
        this.recheck = this.loaded;
        this.restarted = this.events.length > 0;
      }
      this.link = 'connected';
      this.head = view.authority.last_sequence;
      this.restarts = view.authority.restarts;
    } else {
      const settling =
        observation.state === 'pending' ||
        (view !== null &&
          view.reason === null &&
          (view.connection === 'connecting' ||
            view.connection === 'synchronizing'));
      this.link = settling ? 'connecting' : 'offline';
      // Answers in flight belong to a connection that is gone. What is shown stays, and
      // is said to be from before: nothing here is newly canonical.
      this.generation++;
      this.reading = false;
      this.wanted = null;
      this.failedHead = undefined;
      this.dropOlder();
      if (this.loaded) this.stale = true;
    }
    this.publish();
    this.sync();
  }
  /** The Events view is shown. */
  setVisible(visible: boolean) {
    if (this.disposed || this.visible === visible) return;
    this.visible = visible;
    this.sync();
  }
  /** The list is scrolled to its newest end. Only there are new events read in. */
  setAtTop(atTop: boolean) {
    if (this.disposed || this.atTop === atTop) return;
    this.atTop = atTop;
    if (atTop) this.sync();
  }
  private following(): boolean {
    return this.atTop && !this.detached;
  }

  /** Reads when what is shown is behind, only while shown, and never twice at once. */
  private sync() {
    if (this.disposed || this.link !== 'connected' || !this.visible) return;
    // A read in flight looks again when it lands: nothing is queued behind it here.
    if (this.reading) return;
    if (this.failedHead !== undefined && this.failedHead === this.head) return;
    if (!this.loaded || this.recheck) {
      this.readLatest(this.loaded ? 'merge' : 'replace');
      return;
    }
    if (!before(this.shownHead, this.head)) return;
    if (this.following()) {
      if (this.triedHead === this.head) return;
      this.readLatest('merge');
    } else if (!this.newer) {
      // The reader is somewhere older: they are told, and nothing moves under them.
      this.newer = true;
      this.publish();
    }
  }
  private readLatest(mode: 'merge' | 'replace') {
    if (this.reading) {
      this.wanted = mode === 'replace' ? 'replace' : (this.wanted ?? 'merge');
      return;
    }
    const generation = ++this.generation;
    const head = this.head;
    this.triedHead = head;
    this.reading = true;
    this.wanted = null;
    this.publish();
    const settle = (apply: () => void) => {
      if (this.disposed || generation !== this.generation) return;
      this.reading = false;
      apply();
      this.publish();
      const again = this.wanted;
      this.wanted = null;
      if (again) this.readLatest(again);
      else this.sync();
    };
    this.client.latest().then(
      (page) => settle(() => this.show(page, mode)),
      (error) =>
        settle(() => {
          this.readFailure = failureOf(error);
          this.failedHead = head;
        }),
    );
  }
  private show(page: EventsPage, mode: 'merge' | 'replace') {
    this.recheck = false;
    this.readFailure = null;
    this.failedHead = undefined;
    this.stale = false;
    this.loaded = true;
    this.shownHead = page.head;
    const replaced = this.stream !== null && this.stream !== page.stream;
    if (replaced) {
      // Another stream. Its positions say nothing about the ones shown: what was shown
      // is kept apart, below a boundary, and its positions are not used again.
      const kept = this.events.slice(0, MAX_PREVIOUS);
      if (kept.length > 0)
        this.previous = { stream: this.stream!, events: kept };
      this.olderFailure = null;
      this.restarted = false;
    }
    const top = this.events.at(0)?.sequence;
    if (replaced || mode === 'replace' || top === undefined) {
      this.take(page);
      return;
    }
    if (!this.following()) {
      this.newer = page.events.at(0)?.sequence !== top;
      return;
    }
    const at = page.events.findIndex((event) => event.sequence === top);
    // The newest event shown is not in the newest window: more arrived than a window
    // holds, or this history is not the one that was shown. Either way, what is shown
    // is the newest window, whole.
    if (at === -1) {
      this.take(page);
      return;
    }
    const have = new Set(this.events.map((event) => event.sequence));
    const fresh = page.events
      .slice(0, at)
      .filter((event) => !have.has(event.sequence));
    this.events = [...fresh, ...this.events];
    this.newer = false;
    if (this.events.length > MAX_EVENTS) {
      this.events = this.events.slice(0, MAX_EVENTS);
      this.older = this.events.at(-1)!.sequence;
      this.start = 'more';
      this.dropOlder();
    }
  }
  /** An answer about the window before a list that is no longer the one shown. */
  private dropOlder() {
    this.olderEpoch++;
    this.loadingOlder = false;
  }
  private take(page: EventsPage) {
    this.dropOlder();
    this.stream = page.stream;
    this.events = page.events;
    this.older = page.older;
    this.start = startOf(page);
    this.detached = false;
    this.newer = false;
  }

  /**
   * Reads the newest events again. Where the reader is at the newest end they are added
   * to what is shown; anywhere else this is the way back to the newest window.
   */
  refresh() {
    if (this.disposed || this.link !== 'connected') return;
    this.readFailure = null;
    this.failedHead = undefined;
    this.restarted = false;
    this.readLatest(this.following() ? 'merge' : 'replace');
  }
  /** Reads the window before the oldest event shown. */
  loadOlder() {
    const [stream, cursor] = [this.stream, this.older];
    if (this.disposed || this.link !== 'connected' || this.loadingOlder) return;
    if (stream === null || cursor === null || this.reading) return;
    const epoch = this.olderEpoch;
    this.loadingOlder = true;
    this.olderFailure = null;
    this.publish();
    const current = () => !this.disposed && epoch === this.olderEpoch;
    this.client.older(stream, cursor).then(
      (page) => {
        if (!current()) return;
        this.loadingOlder = false;
        const have = new Set(this.events.map((event) => event.sequence));
        this.events = [
          ...this.events,
          ...page.events.filter((event) => !have.has(event.sequence)),
        ];
        this.older = page.older;
        this.start = startOf(page);
        if (this.events.length > MAX_EVENTS) {
          // Room is made at the newest end: the reader is in older history.
          this.events = this.events.slice(this.events.length - MAX_EVENTS);
          this.detached = true;
        }
        this.publish();
      },
      (error) => {
        if (!current()) return;
        this.loadingOlder = false;
        this.olderFailure = failureOf(error);
        // The authority says this is another history: where it is now is read.
        if (this.olderFailure.code === 'changed') this.recheck = true;
        this.publish();
        this.sync();
      },
    );
  }

  select(key: EventKey | null) {
    if (this.disposed) return;
    this.selected = key;
    this.publish();
  }
  setFilter(filter: EventFilter) {
    if (this.disposed) return;
    this.filter = filter;
    this.publish();
  }
  /** Narrows what is shown to the loaded events of one session or one turn. */
  setScope(scope: EventScope | null) {
    if (this.disposed) return;
    this.scope = scope;
    this.publish();
  }
  /** The note that the authority restarted was read. */
  acknowledge() {
    if (this.disposed) return;
    this.restarted = false;
    this.publish();
  }

  dispose() {
    this.disposed = true;
  }
}
