import type {
  ManagedSessionId,
  ManagedTurnId,
} from '../../contracts/identities';
import type { SessionFailure } from '../../contracts/sessions';
import {
  PROMPT_MAX_BYTES,
  TURN_LIST_LIMIT,
  turnEnded,
  type ApprovalNotice,
  type ApprovalOutcome,
  type SessionFacts,
  type TurnEvent,
  type TurnEventKind,
  type TurnList,
  type TurnReceipt,
  type TurnSummary,
  type TurnView,
} from '../../contracts/turns';
import { SessionCallError } from '../../desktop/sessions-client';
import {
  cancelTurn,
  readTurn,
  readTurns,
  startTurn,
} from '../../desktop/turns-client';
import type { AuthorityObservation } from '../../shell/authority';
import { ensureUnique, folderName, shortId } from '../sessions/present';

export type AgentLink = 'connecting' | 'offline' | 'connected';

/** One thing shown under a turn, in the order the authority's stream gave it. */
export type TranscriptItem =
  /** The assistant's answer. Plain text: it is shown as written and never interpreted. */
  | { type: 'text'; text: string }
  /** A mediated action, or an approval being asked for: what happened, and how often. */
  | { type: 'activity'; kind: TurnEventKind; count: number }
  | { type: 'resolved'; outcome: ApprovalOutcome | null }
  /** Output here was not kept by the authority. */
  | { type: 'gap' };

export interface TurnEntry {
  id: ManagedTurnId;
  /** The turn as the authority holds it. Null between its receipt and its first read. */
  turn: TurnSummary | null;
  /**
   * What the user wrote, for a turn started in this window since it opened. Wyss keeps
   * no conversation text: for any other turn there is nothing to show.
   */
  prompt: string | null;
  items: TranscriptItem[];
  /** The model is reasoning. What it reasons is not part of what the window is given. */
  thinking: boolean;
  /** The approval the turn waits on, as last read. Nothing here answers it. */
  approval: ApprovalNotice | null;
  /** A cancel was asked for; the turn has stopped only when `turn` says so. */
  cancel: 'idle' | 'requesting' | 'requested';
  cancelFailure: SessionFailure | null;
  /** The window dropped the oldest output of this turn to stay within its bound. */
  clipped: boolean;
  /** Why reading this turn stopped, until it is read again. */
  readFailure: SessionFailure | null;
}

export interface StartView {
  status: 'pending' | 'failed';
  prompt: string;
  failure?: SessionFailure;
}
export interface AgentSessionView {
  session: ManagedSessionId;
  label: string;
  /** The session as last read; null before the first read on this connection. */
  facts: SessionFacts | null;
  /** Oldest first. */
  turns: TurnEntry[];
  /** Presentation hint from the desktop. The authority decides. */
  runnable: boolean;
  /** Older turns exist that were not read. */
  partial: boolean;
  reading: boolean;
  readFailure: SessionFailure | null;
  draft: string;
  start: StartView | null;
  /** The turn of this session that has not ended, as far as it was read. */
  active: ManagedTurnId | null;
}
export interface AgentState {
  link: AgentLink;
  tabs: { session: ManagedSessionId; label: string }[];
  current: AgentSessionView | null;
  /** A turn of some open session has not ended, as far as it was read. */
  following: boolean;
}

export interface TurnsClient {
  turns(session: ManagedSessionId): Promise<TurnList>;
  read(
    turn: ManagedTurnId,
    after: string | null,
    wait: boolean,
  ): Promise<TurnView>;
  start(
    intent: string,
    session: ManagedSessionId,
    revision: number,
    prompt: string,
  ): Promise<TurnReceipt>;
  cancel(
    intent: string,
    turn: ManagedTurnId,
    revision: number,
  ): Promise<TurnReceipt>;
}
const desktop: TurnsClient = {
  turns: readTurns,
  read: readTurn,
  start: startTurn,
  cancel: cancelTurn,
};

/** Sessions open in the Agent view at once. */
export const MAX_TABS = 8;
/** Characters of one turn's answer the window keeps. Older output is dropped first. */
export const MAX_TURN_CHARS = 200_000;
/** Items of one turn the window keeps. */
export const MAX_TURN_ITEMS = 400;
/** Characters of a draft the composer holds. Far above what a turn can carry. */
export const MAX_DRAFT_CHARS = 4 * PROMPT_MAX_BYTES;
/**
 * The desktop holds a waiting read while there is nothing new. This is only the floor
 * between two readings that brought nothing, whatever the bridge does.
 */
export const FOLLOW_FLOOR_MS = 250;
/** How long a turn that is not over waits before it is read again after a failed read. */
export const FOLLOW_RETRY_MS = 2_000;

interface Entry extends TurnEntry {
  /** Where the next reading starts. */
  cursor: string | null;
  chars: number;
  /** Everything the authority holds for an ended turn was read, or nothing is held. */
  loaded: boolean;
  /** The next piece of text starts a new block. */
  paragraph: boolean;
  /** A list has named this turn. One that was, and no longer is, fell out of it. */
  listed: boolean;
}
interface Start {
  intent: string;
  prompt: string;
  revision: number;
  status: 'pending' | 'failed';
  failure?: SessionFailure;
}
interface Model {
  session: ManagedSessionId;
  label: string;
  /** The label is a stand-in until the session's own folder is read. */
  provisional: boolean;
  facts: SessionFacts | null;
  entries: Entry[];
  runnable: boolean;
  partial: boolean;
  /** Head of the list that is shown; undefined before one was read on this connection. */
  head: string | null | undefined;
  triedHead: string | null;
  triedRestarts: number;
  listing: boolean;
  wanted: boolean;
  listFailure: SessionFailure | null;
  generation: number;
  draft: string;
  start: Start | null;
}

function sequence(text: string | null | undefined): bigint {
  return text ? BigInt(text) : 0n;
}
function failureOf(error: unknown): SessionFailure {
  return error instanceof SessionCallError
    ? error.failure
    : { code: 'internal', retry: false };
}
function same(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}
function over(entry: Entry): boolean {
  return entry.turn !== null && turnEnded(entry.turn.state);
}
/** Whether the text is something a turn can carry: not blank, and within the bound. */
export function promptFits(text: string): boolean {
  return (
    text.trim().length > 0 &&
    new TextEncoder().encode(text).length <= PROMPT_MAX_BYTES
  );
}

/**
 * Presentation state of the Agent view. It decides nothing about turns: it reads what
 * the authority holds, sends the user's intents and reads again.
 *
 * It lives for the application's lifetime, so a turn goes on being read whichever view
 * is shown. One reading is in flight at most, whatever the number of sessions open:
 * there is one follower, and it follows the one turn that has not ended. Nothing is
 * read while no turn is running and nothing changed at the authority.
 *
 * A start is the user's one action: its identity is made once, and only that identity
 * is ever sent again. A start whose answer was lost is not replaced by a new one.
 */
export class AgentController {
  private link: AgentLink = 'connecting';
  private head: string | null = null;
  private restarts = 0;
  private visible = false;
  /** In the order they were opened. */
  private models: Model[] = [];
  private current: ManagedSessionId | null = null;
  /** The one reading in flight. */
  private reading: object | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private disposed = false;

  constructor(
    private readonly update: (state: AgentState) => void,
    private readonly client: TurnsClient = desktop,
    private readonly newIntent: () => string = () => crypto.randomUUID(),
  ) {}

  /**
   * What each open tab is called. Tabs that would read the same take the short identity of
   * their session as well, so two sessions of one project are never one name twice.
   */
  private labels(): Map<ManagedSessionId, string> {
    const count = new Map<string, number>();
    for (const { label } of this.models)
      count.set(label, (count.get(label) ?? 0) + 1);
    return ensureUnique(
      new Map(
        this.models.map(({ session, label }) => [
          session,
          (count.get(label) ?? 0) > 1
            ? `${label} · ${shortId(session)}`
            : label,
        ]),
      ),
    );
  }

  state(): AgentState {
    const model = this.model();
    const labels = this.labels();
    return {
      link: this.link,
      following: this.models.some((m) => m.entries.some((e) => !over(e))),
      tabs: this.models.map(({ session }) => ({
        session,
        label: labels.get(session) ?? '',
      })),
      current: model && {
        session: model.session,
        label: labels.get(model.session) ?? model.label,
        facts: model.facts,
        turns: model.entries.map((entry) => ({
          id: entry.id,
          turn: entry.turn,
          prompt: entry.prompt,
          items: entry.items.map((item) => ({ ...item })),
          thinking: entry.thinking,
          approval: entry.approval,
          cancel: entry.cancel,
          cancelFailure: entry.cancelFailure,
          clipped: entry.clipped,
          readFailure: entry.readFailure,
        })),
        runnable: model.runnable,
        partial: model.partial,
        reading: model.listing,
        readFailure: model.listFailure,
        draft: model.draft,
        start: model.start && {
          status: model.start.status,
          prompt: model.start.prompt,
          failure: model.start.failure,
        },
        active: model.entries.find((entry) => !over(entry))?.id ?? null,
      },
    };
  }
  private publish() {
    if (!this.disposed) this.update(this.state());
  }
  private model(id: ManagedSessionId | null = this.current): Model | null {
    return this.models.find((m) => m.session === id) ?? null;
  }
  private open_(model: Model): boolean {
    return !this.disposed && this.models.includes(model);
  }

  /** The shell's one authority follower feeds this. No second follower exists. */
  observe(observation: AuthorityObservation) {
    if (this.disposed) return;
    const view = observation.state === 'observed' ? observation.view : null;
    if (view?.connection === 'connected' && view.authority) {
      this.link = 'connected';
      this.head = view.authority.last_sequence;
      this.restarts = view.authority.restarts;
      // Whatever changed may be why a reading failed: it is tried again.
      for (const model of this.models)
        for (const entry of model.entries)
          if (!over(entry) || !entry.loaded) entry.readFailure = null;
    } else {
      const settling =
        observation.state === 'pending' ||
        (view !== null &&
          view.reason === null &&
          (view.connection === 'connecting' ||
            view.connection === 'synchronizing'));
      this.link = settling ? 'connecting' : 'offline';
      // What was read stays shown as what was read. Nothing read on a connection
      // that is gone is taken for the present: every answer in flight is dropped.
      this.reading = null;
      this.rest();
      for (const model of this.models) {
        model.generation++;
        model.listing = false;
        model.wanted = false;
        model.head = undefined;
        model.listFailure = null;
        model.runnable = false;
      }
    }
    this.publish();
    this.sync();
    this.pump();
  }
  /** Lists are read only while the view is shown. A running turn is read regardless. */
  setVisible(visible: boolean) {
    if (this.disposed || this.visible === visible) return;
    this.visible = visible;
    this.sync();
  }

  /**
   * Opens a session in the Agent view and shows it. Answers false when every tab is
   * taken by a session with something in flight. A `provisional` label stands in for a
   * session whose folder is not known yet: it is replaced by the folder once it is read.
   */
  open(session: ManagedSessionId, label: string, provisional = false): boolean {
    if (this.disposed) return false;
    const known = this.model(session);
    if (known) {
      // Opened again from where its name is known: that name is the better one.
      if (!provisional) {
        known.label = label;
        known.provisional = false;
      }
    } else {
      if (this.models.length >= MAX_TABS) {
        const idle = this.models.find(
          (m) =>
            m.session !== this.current &&
            m.start === null &&
            m.entries.every(over),
        );
        if (!idle) return false;
        this.drop(idle);
      }
      this.models.push({
        session,
        label,
        provisional,
        facts: null,
        entries: [],
        runnable: false,
        partial: false,
        head: undefined,
        triedHead: null,
        triedRestarts: 0,
        listing: false,
        wanted: false,
        listFailure: null,
        generation: 0,
        draft: '',
        start: null,
      });
    }
    this.current = session;
    this.publish();
    this.sync();
    this.pump();
    return true;
  }
  select(session: ManagedSessionId) {
    if (this.disposed || this.current === session || !this.model(session))
      return;
    this.current = session;
    this.publish();
    this.sync();
    this.pump();
  }
  /**
   * Takes a session out of the view. It asks nothing of the authority: a turn of that
   * session goes on as it was, and is found again when the session is opened.
   */
  close(session: ManagedSessionId) {
    const model = this.model(session);
    // A start in flight is the user's action: its answer is not thrown away.
    if (this.disposed || !model || model.start?.status === 'pending') return;
    const at = this.models.indexOf(model);
    this.drop(model);
    if (this.current === session)
      this.current = (this.models[at] ?? this.models[at - 1])?.session ?? null;
    this.publish();
    this.sync();
    this.pump();
  }
  private drop(model: Model) {
    model.generation++;
    this.models = this.models.filter((m) => m !== model);
  }

  /** Whether the list shown for a session is behind what the authority has. */
  private stale(model: Model): boolean {
    if (model.head === undefined && model.listFailure === null) return true;
    if (model.triedRestarts !== this.restarts) return true;
    if (model.triedHead === this.head) return false;
    return (
      model.head === undefined || sequence(model.head) < sequence(this.head)
    );
  }
  /** Reads the shown session's turns when they are behind, and never twice at once. */
  private sync(force = false, model = this.model()) {
    if (this.disposed || this.link !== 'connected' || !model) return;
    if (!force && (!this.visible || !this.stale(model))) return;
    if (model.listing) {
      model.wanted = true;
      return;
    }
    const generation = ++model.generation;
    model.triedHead = this.head;
    model.triedRestarts = this.restarts;
    model.listing = true;
    model.wanted = false;
    this.publish();
    const settle = (apply: () => void) => {
      if (!this.open_(model) || generation !== model.generation) return;
      model.listing = false;
      apply();
      this.publish();
      const again = model.wanted;
      model.wanted = false;
      if (again) this.sync(true, model);
      this.pump();
    };
    this.client.turns(model.session).then(
      (list) => settle(() => this.merge(model, list)),
      (error) =>
        settle(() => {
          model.listFailure = failureOf(error);
          model.runnable = false;
        }),
    );
  }
  private merge(model: Model, list: TurnList) {
    model.facts = list.session;
    // A stand-in name gives way to the folder the authority says the session is bound to.
    const folder = folderName(list.session.workspace);
    if (model.provisional && folder !== null) {
      model.label = folder;
      model.provisional = false;
    }
    model.runnable = list.runnable;
    model.partial = list.partial;
    model.head = list.head;
    model.listFailure = null;
    const known = new Map(
      model.entries.map((e) => [e.id.toLowerCase(), e] as const),
    );
    const entries: Entry[] = [];
    for (const turn of [...list.turns].reverse()) {
      const entry = known.get(turn.id.toLowerCase()) ?? this.entry(turn.id);
      known.delete(turn.id.toLowerCase());
      entry.listed = true;
      this.record(entry, turn);
      entries.push(entry);
    }
    // Not in the list: a turn started here that the list was read too early to name is
    // kept, after the others. One that was named before fell out of what is listed.
    for (const entry of known.values()) if (!entry.listed) entries.push(entry);
    model.entries = entries.slice(-TURN_LIST_LIMIT);
  }
  private entry(id: ManagedTurnId): Entry {
    return {
      id,
      turn: null,
      prompt: null,
      items: [],
      thinking: false,
      approval: null,
      cancel: 'idle',
      cancelFailure: null,
      clipped: false,
      readFailure: null,
      cursor: null,
      chars: 0,
      loaded: false,
      paragraph: false,
      listed: false,
    };
  }
  /** Takes a record of the turn, unless what is held is already newer. */
  private record(entry: Entry, turn: TurnSummary) {
    if (entry.turn && turn.revision < entry.turn.revision) return;
    entry.turn = turn;
    if (!turnEnded(turn.state)) return;
    entry.thinking = false;
    entry.approval = null;
    entry.cancel = 'idle';
    // Ended, and the authority holds nothing of it: there is nothing more to read.
    // If something of it was read before, what came after that is said to be missing.
    if (!turn.output && !entry.loaded) {
      entry.loaded = true;
      if (entry.cursor !== null) this.push(entry, { type: 'gap' });
    }
  }

  /** Reads again what is shown: the session's turns, and any turn whose reading failed. */
  refresh() {
    const model = this.model();
    if (!model) return;
    model.listFailure = null;
    for (const entry of model.entries) entry.readFailure = null;
    this.rest();
    this.sync(true);
    this.pump();
  }

  /** What to read next: the output still held of the shown session's ended turns,
   *  newest first, then the turn that has not ended, in whichever open session. */
  private next(): { model: Model; entry: Entry; wait: boolean } | null {
    const shown = this.model();
    if (shown)
      for (const entry of [...shown.entries].reverse())
        if (over(entry) && !entry.loaded && entry.readFailure === null)
          return { model: shown, entry, wait: false };
    const models = shown
      ? [shown, ...this.models.filter((m) => m !== shown)]
      : this.models;
    for (const model of models)
      for (const entry of model.entries)
        if (!over(entry) && entry.readFailure === null)
          // The first reading of a turn answers at once: it brings its record.
          return { model, entry, wait: entry.turn !== null };
    return null;
  }
  private rest() {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }
  private later(ms: number) {
    this.rest();
    this.timer = setTimeout(() => {
      this.timer = undefined;
      // A failed reading of a turn that is not over is tried again.
      let cleared = false;
      for (const model of this.models)
        for (const entry of model.entries)
          if (!over(entry) && entry.readFailure?.retry) {
            entry.readFailure = null;
            cleared = true;
          }
      if (cleared) this.publish();
      this.pump();
    }, ms);
  }
  /** The one follower. It never reads two things at once, and never without a reason. */
  private pump() {
    if (
      this.disposed ||
      this.link !== 'connected' ||
      this.reading !== null ||
      this.timer !== undefined
    )
      return;
    const job = this.next();
    if (!job) return;
    const { model, entry, wait } = job;
    const token = {};
    this.reading = token;
    // Still shown: the session is open and the turn is one of its turns.
    const held = () => this.open_(model) && model.entries.includes(entry);
    this.client.read(entry.id, entry.cursor, wait).then(
      (view) => {
        // An answer to anything but the reading in flight is dropped.
        if (this.reading !== token) return;
        this.reading = null;
        if (!held()) {
          this.pump();
          return;
        }
        const before = entry.turn?.state;
        const idle = this.apply(model, entry, view);
        this.publish();
        // It ended: whether another may start is read from the authority.
        if (over(entry) && before !== entry.turn?.state) this.sync(true, model);
        if (idle) this.later(FOLLOW_FLOOR_MS);
        else this.pump();
      },
      (error) => {
        if (this.reading !== token) return;
        this.reading = null;
        if (held()) {
          entry.readFailure = failureOf(error);
          this.publish();
          if (entry.readFailure.retry && !over(entry))
            this.later(FOLLOW_RETRY_MS);
        }
        this.pump();
      },
    );
  }
  /**
   * Takes one reading into a turn. Answers whether it brought nothing and the turn is
   * not over, which is the one case the next reading waits a moment for.
   */
  private apply(model: Model, entry: Entry, view: TurnView): boolean {
    // A turn of another session is never shown under this one.
    if (!same(view.turn.session, model.session)) {
      entry.readFailure = { code: 'internal', retry: false };
      return false;
    }
    let at = sequence(entry.cursor);
    let fresh = 0;
    for (const event of view.events) {
      const position = BigInt(event.sequence);
      // Shown already: a position is taken in once.
      if (position <= at) continue;
      // A step over positions that nothing accounts for is output that is missing.
      if (position > at + 1n && event.kind !== 'gap')
        this.push(entry, { type: 'gap' });
      at = position;
      fresh++;
      this.show(entry, event);
    }
    if (view.next !== null && sequence(view.next) >= sequence(entry.cursor))
      entry.cursor = view.next;
    this.record(entry, view.turn);
    if (!over(entry)) entry.approval = view.approval;
    // Ended and read to its end. A reading that brought nothing ends it too, whatever
    // it says is left: an ended turn is never read in a loop.
    if (over(entry) && (!view.more || fresh === 0)) entry.loaded = true;
    return fresh === 0 && !over(entry);
  }
  private show(entry: Entry, event: TurnEvent) {
    switch (event.kind) {
      case 'text': {
        entry.thinking = false;
        const text = event.text ?? '';
        if (text === '') return;
        const last = entry.items.at(-1);
        if (last?.type === 'text' && !entry.paragraph) last.text += text;
        else this.push(entry, { type: 'text', text });
        entry.paragraph = false;
        entry.chars += text.length;
        this.bound(entry);
        return;
      }
      case 'thinking':
        entry.thinking = true;
        return;
      case 'message_end':
        entry.thinking = false;
        entry.paragraph = true;
        return;
      case 'started':
      case 'completed':
      case 'interrupted':
      case 'failed':
        // The lifecycle is the record's to say, not the stream's.
        entry.thinking = false;
        return;
      case 'gap':
        this.push(entry, { type: 'gap' });
        return;
      case 'approval_resolved':
        this.push(entry, {
          type: 'resolved',
          outcome: event.text as ApprovalOutcome | null,
        });
        return;
      default: {
        const last = entry.items.at(-1);
        if (last?.type === 'activity' && last.kind === event.kind) last.count++;
        else this.push(entry, { type: 'activity', kind: event.kind, count: 1 });
      }
    }
  }
  private push(entry: Entry, item: TranscriptItem) {
    const last = entry.items.at(-1);
    // One mark for one stretch of missing output.
    if (item.type === 'gap' && last?.type === 'gap') return;
    entry.items.push(item);
    entry.paragraph = false;
    this.bound(entry);
  }
  /** Keeps a turn within what the window holds of it: the oldest output goes first. */
  private bound(entry: Entry) {
    while (entry.items.length > MAX_TURN_ITEMS) {
      const gone = entry.items.shift()!;
      if (gone.type === 'text') entry.chars -= gone.text.length;
      entry.clipped = true;
    }
    while (entry.chars > MAX_TURN_CHARS) {
      const first = entry.items.find((item) => item.type === 'text');
      if (first?.type !== 'text') break;
      const excess = entry.chars - MAX_TURN_CHARS;
      if (first.text.length > excess) {
        first.text = first.text.slice(excess);
        entry.chars -= excess;
      } else {
        entry.chars -= first.text.length;
        entry.items.splice(entry.items.indexOf(first), 1);
      }
      entry.clipped = true;
    }
  }

  /** What the composer holds for the shown session. Each session has its own. */
  setDraft(text: string) {
    const model = this.model();
    if (this.disposed || !model) return;
    model.draft = text.slice(0, MAX_DRAFT_CHARS);
    this.publish();
  }
  /**
   * Starts a turn with what the composer holds. One at a time: while a start is
   * pending, or failed and neither retried nor dismissed, nothing else is sent.
   */
  start() {
    const model = this.model();
    if (this.disposed || this.link !== 'connected' || !model) return;
    if (model.start !== null || model.facts === null) return;
    // Presentation: what was read says no turn can start now. The authority decides
    // every start that is sent; this only keeps one from being sent for nothing.
    if (!model.runnable || !model.entries.every(over)) return;
    if (!promptFits(model.draft)) return;
    model.start = {
      intent: this.newIntent(),
      prompt: model.draft,
      // The revision the user was shown: the authority refuses if it moved on.
      revision: model.facts.revision,
      status: 'pending',
    };
    model.draft = '';
    this.dispatch(model, model.start);
  }
  /** The failed start again, whole: the same identity, the same input, the same turn. */
  retryStart() {
    const model = this.model();
    const start = model?.start;
    if (this.disposed || this.link !== 'connected' || !model || !start) return;
    if (start.status !== 'failed' || !start.failure?.retry) return;
    this.dispatch(model, start);
  }
  /** Gives the input back to the composer. Whether a turn started is read, not assumed. */
  dismissStart() {
    const model = this.model();
    if (this.disposed || model?.start?.status !== 'failed') return;
    if (model.draft === '') model.draft = model.start.prompt;
    model.start = null;
    this.publish();
    this.sync(true);
  }
  private dispatch(model: Model, start: Start) {
    start.status = 'pending';
    start.failure = undefined;
    this.publish();
    this.client
      .start(start.intent, model.session, start.revision, start.prompt)
      .then(
        (receipt) => {
          if (!this.open_(model) || model.start !== start) return;
          model.start = null;
          let entry = model.entries.find((e) => same(e.id, receipt.id));
          if (!entry) {
            entry = this.entry(receipt.id);
            model.entries = [...model.entries, entry].slice(-TURN_LIST_LIMIT);
          }
          entry.prompt = start.prompt;
          this.publish();
          this.sync(true, model);
          this.pump();
        },
        (error) => {
          if (!this.open_(model) || model.start !== start) return;
          start.status = 'failed';
          start.failure = failureOf(error);
          this.publish();
          // A refusal usually means the session is not what was shown: read it again.
          this.sync(true, model);
        },
      );
  }

  /**
   * Asks the authority to stop a turn at the revision that is shown. The turn is shown
   * as stopping until a reading says what became of it.
   */
  cancel(turn: ManagedTurnId) {
    const model = this.model();
    const entry = model?.entries.find((e) => e.id === turn);
    if (this.disposed || this.link !== 'connected' || !model || !entry?.turn)
      return;
    if (over(entry) || entry.cancel !== 'idle') return;
    entry.cancel = 'requesting';
    entry.cancelFailure = null;
    this.publish();
    this.client.cancel(this.newIntent(), entry.id, entry.turn.revision).then(
      () => {
        if (!this.open_(model) || !model.entries.includes(entry)) return;
        if (entry.cancel === 'requesting') entry.cancel = 'requested';
        this.publish();
        this.pump();
      },
      (error) => {
        if (!this.open_(model) || !model.entries.includes(entry)) return;
        // Not taken, or not known to be: the turn is whatever a reading says it is.
        if (entry.cancel === 'requesting') entry.cancel = 'idle';
        if (!over(entry)) entry.cancelFailure = failureOf(error);
        this.publish();
        this.sync(true, model);
        this.pump();
      },
    );
  }

  dispose() {
    this.disposed = true;
    this.reading = null;
    this.rest();
  }
}
