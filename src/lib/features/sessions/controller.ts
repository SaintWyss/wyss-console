import type { ManagedSessionId } from '../../contracts/identities';
import type {
  ActivationProgress,
  ActivationStage,
  SessionFailure,
  SessionPage,
  SessionReceipt,
  SessionSummary,
} from '../../contracts/sessions';
import {
  abandonSession,
  activateSession,
  chooseWorkspace,
  closeSession,
  prepareSession,
  readSessionsPage,
  resumeSession,
  SessionCallError,
} from '../../desktop/sessions-client';
import type { AuthorityObservation } from '../../shell/authority';

export type SessionsLink = 'connecting' | 'offline' | 'connected';
export type OperationKind = 'activate' | 'resume' | 'close' | 'abandon';
/**
 * What an activation is doing now. Each one is justified by something that was read:
 * the request in flight, or what the authority last answered or reported.
 */
export type ActivationStep = 'preparing' | 'starting' | 'binding' | 'attaching';

export interface SessionOperation {
  kind: OperationKind;
  /** The session acted on; null while a new one is being prepared. */
  session: ManagedSessionId | null;
  status: 'pending' | 'failed';
  failure?: SessionFailure;
  /** Only for an activation that is pending. */
  step?: ActivationStep;
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
  /** The existing session the composer activates; null when it creates a new one. */
  composeFor: ManagedSessionId | null;
  /** The folder the user selected for the composer, as the native dialog returned it. */
  workspace: string | null;
  /** The native folder dialog is open. */
  choosing: boolean;
  chooseFailure: SessionFailure | null;
  confirmingClose: ManagedSessionId | null;
  confirmingDiscard: ManagedSessionId | null;
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
  activate(
    operation: string,
    session: ManagedSessionId,
    revision: number,
    workspace: string,
  ): Promise<ActivationProgress>;
  abandon(intent: string, session: ManagedSessionId): Promise<SessionReceipt>;
  chooseWorkspace(): Promise<string | null>;
}
const desktop: SessionsClient = {
  page: readSessionsPage,
  prepare: prepareSession,
  resume: resumeSession,
  close: closeSession,
  activate: activateSession,
  abandon: abandonSession,
  chooseWorkspace,
};

/** Older pages the window walks back through. Two thousand sessions, as the desktop does. */
const MAX_DEPTH = 80;
/**
 * How long one activation waits, in all, for the authority to bring the runtime up or
 * to finish a binding. The authority's own limits for both are far below it.
 */
export const ACTIVATION_WAIT_MS = 60_000;
/**
 * The authority view is what ends a wait. This is only the longest a wait goes without
 * asking again, should a change never be reported.
 */
export const ACTIVATION_RECHECK_MS = 5_000;

/** What a user action sends, kept whole so a retry sends exactly the same thing. */
interface Intent {
  kind: OperationKind;
  id: string;
  /** Null until the new session an activation asked for exists. */
  session: ManagedSessionId | null;
  revision: number;
  workspace?: string;
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
 *
 * An activation is one user action made of several steps the desktop takes one at a
 * time. This controller only repeats the same request until the desktop answers `ready`,
 * waiting for the authority view to change whenever the answer is to wait. It lives for
 * the application's lifetime, so an activation goes on whichever view is shown.
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
  private composeFor: ManagedSessionId | null = null;
  private workspace: string | null = null;
  private choosing = false;
  private chooseFailure: SessionFailure | null = null;
  private confirmingClose: ManagedSessionId | null = null;
  private confirmingDiscard: ManagedSessionId | null = null;
  private operation: SessionOperation | null = null;
  private intent: Intent | null = null;
  /** Ends the wait of the activation in flight, when it is waiting. */
  private wake: (() => void) | null = null;
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
      composeFor: this.composeFor,
      workspace: this.workspace,
      choosing: this.choosing,
      chooseFailure: this.chooseFailure,
      confirmingClose: this.confirmingClose,
      confirmingDiscard: this.confirmingDiscard,
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
      this.confirmingDiscard = null;
    }
    this.publish();
    this.sync();
    // Whatever changed is what a waiting activation was waiting to ask about.
    this.wake?.();
  }
  /** Reads happen only while the view is shown. */
  setVisible(visible: boolean) {
    if (this.disposed || this.visible === visible) return;
    this.visible = visible;
    if (!visible) {
      this.composing = false;
      this.confirmingClose = null;
      this.confirmingDiscard = null;
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
    if (
      this.confirmingDiscard &&
      !this.find(this.confirmingDiscard)?.recoverable
    )
      this.confirmingDiscard = null;
    // The session the composer was opened for is no longer one to activate.
    if (
      this.composing &&
      this.composeFor &&
      !this.find(this.composeFor)?.activatable
    )
      this.composing = false;
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
    this.confirmingDiscard = null;
    this.publish();
    this.sync(true);
  }

  select(id: ManagedSessionId | null) {
    if (this.selected === id) return;
    this.selected = id;
    this.confirmingClose = null;
    this.confirmingDiscard = null;
    this.publish();
  }
  /**
   * Opens the composer for a new session, or for an existing one that has no workspace
   * yet, and closes it. It always opens without a folder: one is chosen each time.
   */
  compose(open: boolean, session: ManagedSessionId | null = null) {
    if (open) {
      if (this.link !== 'connected' || this.busy() || this.choosing) return;
      if (session !== null && !this.find(session)?.activatable) return;
      this.composeFor = session;
      this.workspace = null;
      this.chooseFailure = null;
    }
    this.composing = open;
    this.publish();
  }
  /**
   * Asks the desktop for its native folder dialog. The folder comes back from the
   * desktop, which remembers it: the window never names a path the user did not select.
   */
  chooseWorkspace() {
    if (this.disposed || !this.composing || this.choosing || this.busy())
      return;
    this.choosing = true;
    this.chooseFailure = null;
    this.publish();
    this.client.chooseWorkspace().then(
      (path) => {
        if (this.disposed) return;
        this.choosing = false;
        // Dismissed, or answered for a composer that is gone: nothing was chosen.
        if (path !== null && this.composing) this.workspace = path;
        this.publish();
      },
      (error) => {
        if (this.disposed) return;
        this.choosing = false;
        this.chooseFailure = failureOf(error);
        this.publish();
      },
    );
  }
  /** Asks before a close: it is the one action that cannot be taken back. */
  confirmClose(id: ManagedSessionId | null) {
    if (id !== null && (!this.find(id)?.closable || this.busy())) return;
    this.confirmingClose = id;
    this.confirmingDiscard = null;
    this.publish();
  }
  /** Asks before giving up on an uncertain attempt: what Codex did stays unknown. */
  confirmDiscard(id: ManagedSessionId | null) {
    if (id !== null && (!this.find(id)?.recoverable || this.busy())) return;
    this.confirmingDiscard = id;
    this.confirmingClose = null;
    this.publish();
  }

  /** Creates and activates, or activates, what the composer holds. */
  activate() {
    if (!this.composing || this.workspace === null || this.choosing) return;
    const session = this.composeFor;
    const shown = session === null ? undefined : this.find(session);
    if (session !== null && !shown?.activatable) return;
    this.send({
      kind: 'activate',
      session,
      // A session that does not exist yet will be at the revision it is prepared at.
      revision: shown?.revision ?? 1,
      workspace: this.workspace,
    });
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
  /** Gives up on the attempt the authority holds as uncertain for this session. */
  discard(id: ManagedSessionId) {
    const session = this.find(id);
    if (session?.recoverable)
      this.send({ kind: 'abandon', session: id, revision: session.revision });
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
    this.composing = false;
    this.confirmingClose = null;
    this.confirmingDiscard = null;
    if (intent.kind === 'activate') {
      void this.activation(intent);
      return;
    }
    this.operation = {
      kind: intent.kind,
      session: intent.session,
      status: 'pending',
    };
    this.publish();
    const { client } = this;
    const session = intent.session!;
    const call =
      intent.kind === 'resume'
        ? client.resume(intent.id, session, intent.revision)
        : intent.kind === 'close'
          ? client.close(intent.id, session, intent.revision)
          : client.abandon(intent.id, session);
    call.then(
      () => {
        if (this.disposed || this.intent !== intent) return;
        this.operation = null;
        this.intent = null;
        this.publish();
        this.sync(true);
      },
      (error) => this.fail(intent, error),
    );
  }
  private fail(intent: Intent, error: unknown) {
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
  }

  /**
   * One activation, from the session it may have to create to `ready`. Every request
   * carries the action's one identity, so the same step asked again (by this loop, or
   * by a retry after an unknown outcome) is the same intent for the authority.
   */
  private async activation(intent: Intent) {
    const live = () => !this.disposed && this.intent === intent;
    const doing = (step: ActivationStep) => {
      this.operation = {
        kind: 'activate',
        session: intent.session,
        status: 'pending',
        step,
      };
      this.publish();
    };
    try {
      if (intent.session === null) {
        doing('preparing');
        const receipt = await this.client.prepare(intent.id);
        if (!live()) return;
        // Kept on the action: a retry goes on from the session it already has.
        intent.session = receipt.id;
        // The new session is on the newest page. What it is comes from the next read.
        this.selected = receipt.id;
        if (this.cursors.length) this.forget();
        this.sync(true);
      }
      const session = intent.session;
      const until = Date.now() + ACTIVATION_WAIT_MS;
      let last: ActivationStage | null = null;
      for (;;) {
        doing(this.step(session, last));
        const { stage } = await this.client.activate(
          intent.id,
          session,
          intent.revision,
          intent.workspace!,
        );
        if (!live()) return;
        if (stage === 'ready') break;
        last = stage;
        if (stage === 'bound') {
          // Bound, not attached yet: that much is already true, so it is read.
          this.sync(true);
          continue;
        }
        doing(this.step(session, last));
        const waited = await this.wait(until);
        if (!live()) return;
        // The limit was reached with the authority still saying "wait".
        if (waited === 'expired')
          throw new SessionCallError({
            code: 'runtime_unavailable',
            retry: true,
          });
      }
      this.operation = null;
      this.intent = null;
      this.publish();
      this.sync(true);
    } catch (error) {
      this.fail(intent, error);
    }
  }
  /**
   * What the next request is for, from what was read: the last answer, the page, and
   * the runtime as the authority view reports it. It labels; it decides nothing.
   */
  private step(
    session: ManagedSessionId,
    last: ActivationStage | null,
  ): ActivationStep {
    if (last === 'bound') return 'attaching';
    if (last === 'creating') return 'binding';
    const binding = this.find(session)?.binding;
    if (binding === 'bound_detached' || binding === 'bound_attached')
      return 'attaching';
    return this.agentReady ? 'binding' : 'starting';
  }
  /** Until the authority view changes, or a while has passed, or the limit is reached. */
  private wait(until: number): Promise<'woken' | 'expired'> {
    return new Promise((resolve) => {
      const left = until - Date.now();
      if (left <= 0) {
        resolve('expired');
        return;
      }
      const timer = setTimeout(
        () => {
          this.wake = null;
          resolve('woken');
        },
        Math.min(left, ACTIVATION_RECHECK_MS),
      );
      this.wake = () => {
        clearTimeout(timer);
        this.wake = null;
        resolve('woken');
      };
    });
  }

  dispose() {
    this.disposed = true;
    // A wait in flight ends; its activation sees it was disposed and sends nothing.
    this.wake?.();
  }
}
