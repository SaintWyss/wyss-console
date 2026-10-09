import type { ManagedSessionId } from '../../contracts/identities';
import type {
  SessionFailure,
  SessionPage,
  SessionReceipt,
  SessionSummary,
} from '../../contracts/sessions';
import {
  closeSession,
  prepareSession,
  readSessionsPage,
  resumeSession,
  SessionCallError,
} from '../../desktop/sessions-client';
import type { AuthorityObservation } from '../../shell/authority';

export type SessionsLink = 'connecting' | 'offline' | 'connected';
export type OperationKind = 'prepare' | 'resume' | 'close';

export interface SessionOperation {
  kind: OperationKind;
  /** The session acted on; null while a new one is being prepared. */
  session: ManagedSessionId | null;
  status: 'pending' | 'failed';
  failure?: SessionFailure;
}

/** Everything the Sessions view renders. Canonical data is only ever a page that was read. */
export interface SessionsState {
  link: SessionsLink;
  /** Null while offline, before the first read and while another page is being read. */
  page: SessionPage | null;
  reading: boolean;
  /** Why the last read failed, while there is no newer page to show instead. */
  readFailure: SessionFailure | null;
  /** How many pages older than the newest one this is. */
  depth: number;
  selected: ManagedSessionId | null;
  composing: boolean;
  confirmingClose: ManagedSessionId | null;
  operation: SessionOperation | null;
  /** Whether the managed runtime is ready, as the authority reports it. A hint only. */
  agentReady: boolean;
}

export interface SessionsClient {
  page(before: string | null): Promise<SessionPage>;
  prepare(intent: string): Promise<SessionReceipt>;
  resume(
    intent: string,
    session: ManagedSessionId,
    revision: number,
  ): Promise<SessionReceipt>;
  close(
    intent: string,
    session: ManagedSessionId,
    revision: number,
  ): Promise<SessionReceipt>;
}
const desktop: SessionsClient = {
  page: readSessionsPage,
  prepare: prepareSession,
  resume: resumeSession,
  close: closeSession,
};

/** Older pages the window walks back through. Two thousand sessions, as the desktop does. */
const MAX_DEPTH = 80;

/** What a user action sends, kept whole so a retry sends exactly the same thing. */
interface Intent {
  kind: OperationKind;
  id: string;
  session: ManagedSessionId | null;
  revision: number;
}

function sequence(text: string | null | undefined): bigint {
  return text ? BigInt(text) : 0n;
}
function failureOf(error: unknown): SessionFailure {
  return error instanceof SessionCallError
    ? error.failure
    : { code: 'internal', retry: false };
}

/**
 * Presentation state of the Sessions view. It decides nothing about sessions: it reads
 * pages, sends the user's intents and re-reads. One read and one intent are in flight at
 * most. The authority observation it is given is the wake signal: a page is re-read when
 * the authority's durable head has moved past the head the page was read at.
 */
export class SessionsController {
  private link: SessionsLink = 'connecting';
  private head: string | null = null;
  private restarts = 0;
  private agentReady = false;
  private visible = false;
  private page: SessionPage | null = null;
  /** The authority head and instance count the last read was started for. */
  private triedHead: string | null = null;
  private triedRestarts = 0;
  private reading = false;
  private wanted = false;
  private readFailure: SessionFailure | null = null;
  /** Cursors of the older pages walked into; empty on the newest page. */
  private cursors: string[] = [];
  private selected: ManagedSessionId | null = null;
  private composing = false;
  private confirmingClose: ManagedSessionId | null = null;
  private operation: SessionOperation | null = null;
  private intent: Intent | null = null;
  /** Reads issued so far. An answer to anything but the latest is discarded. */
  private generation = 0;
  private disposed = false;

  constructor(
    private readonly update: (state: SessionsState) => void,
    private readonly client: SessionsClient = desktop,
    private readonly newIntent: () => string = () => crypto.randomUUID(),
  ) {}

  state(): SessionsState {
    return {
      link: this.link,
      page: this.page,
      reading: this.reading,
      readFailure: this.readFailure,
      depth: this.cursors.length,
      selected: this.selected,
      composing: this.composing,
      confirmingClose: this.confirmingClose,
      operation: this.operation,
      agentReady: this.agentReady,
    };
  }
  private publish() {
    if (!this.disposed) this.update(this.state());
  }
  private find(id: ManagedSessionId | null): SessionSummary | undefined {
    return this.page?.sessions.find((s) => s.id === id);
  }
  private busy(): boolean {
    return this.operation?.status === 'pending';
  }

  /** The shell's one authority follower feeds this. No second follower exists. */
  observe(observation: AuthorityObservation) {
    if (this.disposed) return;
    const view = observation.state === 'observed' ? observation.view : null;
    if (view?.connection === 'connected' && view.authority) {
      this.link = 'connected';
      this.head = view.authority.last_sequence;
      this.restarts = view.authority.restarts;
      this.agentReady = view.agent?.actual === 'ready';
    } else {
      const settling =
        observation.state === 'pending' ||
        (view !== null &&
          view.reason === null &&
          (view.connection === 'connecting' ||
            view.connection === 'synchronizing'));
      this.link = settling ? 'connecting' : 'offline';
      this.agentReady = false;
      // Nothing canonical outlives the connection it was read on.
      this.forget();
      this.composing = false;
      this.confirmingClose = null;
    }
    this.publish();
    this.sync();
  }
  /** Reads happen only while the view is shown. */
  setVisible(visible: boolean) {
    if (this.disposed || this.visible === visible) return;
    this.visible = visible;
    if (!visible) {
      this.composing = false;
      this.confirmingClose = null;
      this.publish();
    }
    this.sync();
  }
  private forget() {
    this.page = null;
    this.readFailure = null;
    this.cursors = [];
    this.generation++;
    this.reading = false;
    this.wanted = false;
  }
  /**
   * Whether what is shown is behind what the authority has. One read is made for each
   * head the authority reports: a peer whose pages never catch up is not read in a loop.
   */
  private stale(): boolean {
    if (this.page === null && this.readFailure === null) return true;
    if (this.triedRestarts !== this.restarts) return true;
    if (this.triedHead === this.head) return false;
    return this.page === null || sequence(this.page.head) < sequence(this.head);
  }
  /** Reads when what is shown is behind, and never twice at once. */
  private sync(force = false) {
    if (this.disposed || this.link !== 'connected' || !this.visible) return;
    if (!force && !this.stale()) return;
    if (this.reading) {
      this.wanted = true;
      return;
    }
    const generation = ++this.generation;
    this.triedHead = this.head;
    this.triedRestarts = this.restarts;
    this.reading = true;
    this.wanted = false;
    this.publish();
    this.client.page(this.cursors.at(-1) ?? null).then(
      (page) => this.settle(generation, () => this.show(page)),
      (error) =>
        this.settle(generation, () => {
          // What was shown is behind an authority that could not be read: it goes.
          this.page = null;
          this.readFailure = failureOf(error);
        }),
    );
  }
  private settle(generation: number, apply: () => void) {
    if (this.disposed || generation !== this.generation) return;
    this.reading = false;
    apply();
    this.publish();
    const again = this.wanted;
    this.wanted = false;
    this.sync(again);
  }
  private show(page: SessionPage) {
    this.page = page;
    this.readFailure = null;
    if (this.confirmingClose && !this.find(this.confirmingClose)?.closable)
      this.confirmingClose = null;
  }

  /** Reads again whatever is shown. */
  refresh() {
    this.readFailure = null;
    this.sync(true);
  }
  older() {
    const cursor = this.page?.older;
    if (!cursor || this.cursors.length >= MAX_DEPTH) return;
    this.turn([...this.cursors, cursor]);
  }
  newer() {
    if (this.cursors.length) this.turn(this.cursors.slice(0, -1));
  }
  newest() {
    if (this.cursors.length) this.turn([]);
  }
  private turn(cursors: string[]) {
    // One page read at a time: a turn waits for the read in flight.
    if (this.link !== 'connected' || this.reading) return;
    this.forget();
    this.cursors = cursors;
    this.confirmingClose = null;
    this.publish();
    this.sync(true);
  }

  select(id: ManagedSessionId | null) {
    if (this.selected === id) return;
    this.selected = id;
    this.confirmingClose = null;
    this.publish();
  }
  compose(open: boolean) {
    if (open && (this.link !== 'connected' || this.busy())) return;
    this.composing = open;
    this.publish();
  }
  /** Asks before a close: it is the one action that cannot be taken back. */
  confirmClose(id: ManagedSessionId | null) {
    if (id !== null && (!this.find(id)?.closable || this.busy())) return;
    this.confirmingClose = id;
    this.publish();
  }

  prepare() {
    this.send({ kind: 'prepare', session: null, revision: 0 });
  }
  resume(id: ManagedSessionId) {
    const session = this.find(id);
    if (session)
      this.send({ kind: 'resume', session: id, revision: session.revision });
  }
  close(id: ManagedSessionId) {
    const session = this.find(id);
    if (session)
      this.send({ kind: 'close', session: id, revision: session.revision });
  }
  /** The failed action again, with the identity it had: the authority treats it as one. */
  retry() {
    if (this.operation?.status !== 'failed' || !this.operation.failure?.retry)
      return;
    if (this.intent) this.dispatch(this.intent);
  }
  dismiss() {
    if (this.operation?.status !== 'failed') return;
    this.operation = null;
    this.intent = null;
    this.publish();
  }

  private send(action: Omit<Intent, 'id'>) {
    // One action at a time. A second click while one is pending sends nothing; the
    // authority's own idempotency is what makes a resent intent safe.
    if (this.disposed || this.link !== 'connected' || this.busy()) return;
    this.dispatch({ ...action, id: this.newIntent() });
  }
  private dispatch(intent: Intent) {
    // Reached by a new action, which `send` admits one at a time, or by a retry.
    if (this.link !== 'connected') return;
    this.intent = intent;
    this.operation = {
      kind: intent.kind,
      session: intent.session,
      status: 'pending',
    };
    this.composing = false;
    this.confirmingClose = null;
    this.publish();
    const { client } = this;
    const call =
      intent.kind === 'prepare'
        ? client.prepare(intent.id)
        : intent.kind === 'resume'
          ? client.resume(intent.id, intent.session!, intent.revision)
          : client.close(intent.id, intent.session!, intent.revision);
    call.then(
      (receipt) => {
        if (this.disposed || this.intent !== intent) return;
        this.operation = null;
        this.intent = null;
        if (intent.kind === 'prepare') {
          // The new session is on the newest page. What it is comes from the next read.
          this.selected = receipt.id;
          if (this.cursors.length) this.forget();
        }
        this.publish();
        this.sync(true);
      },
      (error) => {
        if (this.disposed || this.intent !== intent) return;
        this.operation = {
          kind: intent.kind,
          session: intent.session,
          status: 'failed',
          failure: failureOf(error),
        };
        this.publish();
        // A refusal usually means the session is not what was shown: read it again.
        this.sync(true);
      },
    );
  }

  dispose() {
    this.disposed = true;
  }
}
