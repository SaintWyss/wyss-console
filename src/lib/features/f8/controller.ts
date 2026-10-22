import type { ApprovalDetail } from '../../contracts/f8';
import type {
  AuditInfo,
  AuthorityInfo,
  F8Failure,
  F8State,
  Fence,
  LeaseScope,
  LinkInfo,
  ModelsInfo,
  SessionDetail,
  SessionRow,
  SessionsView,
  TurnInfo,
  TurnReading,
  WatchInfo,
} from '../../contracts/f8';
import { LEASE_SCOPES, LEASE_TTLS } from '../../contracts/f8';
import type { ManagedSessionId } from '../../contracts/identities';
import type { AgentMode } from '../../contracts/sessions';
import { turnEnded, type ApprovalNotice } from '../../contracts/turns';
import * as desktop from '../../desktop/f8-client';
import { F8CallError, type Decision } from '../../desktop/f8-client';
import { chooseWorkspace } from '../../desktop/sessions-client';
import { appendEvents, emptyTranscript, type Transcript } from './transcript';

/** The window's calls. The desktop's own, unless a test brings another. */
export interface F8Client {
  state(): Promise<F8State>;
  reconnect(): Promise<F8State>;
  sessions(before: string | null): Promise<SessionsView>;
  session(id: ManagedSessionId): Promise<SessionDetail>;
  prepare(intent: string): Promise<{ id: ManagedSessionId; authority: string }>;
  close(intent: string, fence: Fence, revision: number): Promise<unknown>;
  startRuntime(
    intent: string,
    session: ManagedSessionId,
    revision: number,
    generation: number,
    mode: AgentMode,
    workspace: string | null,
  ): Promise<unknown>;
  stopRuntime(intent: string, fence: Fence, revision: number): Promise<unknown>;
  models(fence: Fence): Promise<ModelsInfo>;
  grantLease(
    intent: string,
    fence: Fence,
    sessionRevision: number,
    scopes: readonly LeaseScope[],
    ttl: number,
  ): Promise<unknown>;
  renewLease(
    intent: string,
    fence: Fence,
    lease: string,
    revision: number,
    ttl: number,
  ): Promise<unknown>;
  revokeLease(
    intent: string,
    lease: string,
    revision: number,
  ): Promise<unknown>;
  createBinding(
    intent: string,
    fence: Fence,
    sessionRevision: number,
    workspace: string,
  ): Promise<unknown>;
  resumeBinding(
    intent: string,
    fence: Fence,
    sessionRevision: number,
  ): Promise<unknown>;
  abandonBinding(
    intent: string,
    fence: Fence,
    operation: string,
  ): Promise<unknown>;
  startTurn(
    intent: string,
    fence: Fence,
    sessionRevision: number,
    prompt: string,
  ): Promise<{ id: string }>;
  cancelTurn(
    intent: string,
    fence: Fence,
    turn: string,
    revision: number,
  ): Promise<unknown>;
  readTurn(
    turn: string,
    after: string | null,
    wait: boolean,
  ): Promise<TurnReading>;
  readApproval(
    approval: string,
    file: number | null,
    offset: number | null,
  ): Promise<ApprovalDetail>;
  decide(
    decision: Decision,
    intent: string,
    fence: Fence,
    approval: string,
    revision: number,
  ): Promise<unknown>;
  audit(
    older: { stream_id: string; before: string } | null,
  ): Promise<AuditInfo>;
  wait(stream: string, after: string | null, ms: number): Promise<WatchInfo>;
  chooseWorkspace(): Promise<string | null>;
}
const live: F8Client = {
  state: desktop.readF8State,
  reconnect: desktop.reconnectF8,
  sessions: desktop.readSessions,
  session: desktop.readSession,
  prepare: desktop.prepareSession,
  close: desktop.closeSession,
  startRuntime: desktop.startRuntime,
  stopRuntime: desktop.stopRuntime,
  models: desktop.readModels,
  grantLease: desktop.grantLease,
  renewLease: desktop.renewLease,
  revokeLease: desktop.revokeLease,
  createBinding: desktop.createBinding,
  resumeBinding: desktop.resumeBinding,
  abandonBinding: desktop.abandonBinding,
  startTurn: desktop.startTurn,
  cancelTurn: desktop.cancelTurn,
  readTurn: desktop.readTurn,
  readApproval: desktop.readApproval,
  decide: desktop.decideApproval,
  audit: desktop.readAudit,
  wait: desktop.waitAudit,
  chooseWorkspace,
};

export type F8Gate = 'unknown' | 'inactive' | 'active';
export type F8Tab = 'overview' | 'conversation' | 'models' | 'history';
export type OperationKind =
  | 'prepare'
  | 'close'
  | 'start'
  | 'stop'
  | 'grant'
  | 'renew'
  | 'revoke'
  | 'bind'
  | 'resume'
  | 'abandon'
  | 'turn'
  | 'cancel';
export type OperationStep = 'starting' | 'stopping' | 'binding';

export interface Operation {
  kind: OperationKind;
  session: ManagedSessionId | null;
  status: 'pending' | 'failed';
  step: OperationStep | null;
  failure: F8Failure | null;
}
export type Notice = 'authority_changed' | 'stale' | null;

/** One file of a change set, as far as its diff was read. */
export interface FileCard {
  index: number;
  path: string;
  kind: 'add' | 'delete' | 'update' | 'move';
  move_path: string | null;
  bytes: number;
  text: string;
  next: number | null;
  reading: boolean;
  failure: F8Failure | null;
}
export interface DecisionView {
  kind: Decision;
  status: 'pending' | 'failed';
  failure: F8Failure | null;
}
export interface ApprovalView {
  detail: ApprovalDetail;
  files: FileCard[];
  listing: boolean;
  decision: DecisionView | null;
}
export interface Conversation {
  turn: string;
  /** The user opened this turn. It is not left for the turn that runs unless they ask. */
  explicit: boolean;
  info: TurnInfo | null;
  transcript: Transcript;
  cursor: string | null;
  /** The follower is reading, and the turn has not been seen to end. */
  following: boolean;
  /** A read failed and the follower is trying again. */
  reconnecting: boolean;
  /** The turn can no longer be followed: the authority does not have it, or changed. */
  lost: boolean;
  /** Output of this turn was not kept by the authority. */
  gap: boolean;
  approval: ApprovalNotice | null;
}
export interface ModelsState {
  status: 'idle' | 'reading' | 'ready' | 'failed';
  info: ModelsInfo | null;
  failure: F8Failure | null;
}
export interface HistoryState {
  status: 'idle' | 'reading' | 'ready' | 'failed';
  page: AuditInfo | null;
  failure: F8Failure | null;
  depth: number;
}
export interface Composer {
  mode: AgentMode;
  workspace: string | null;
  choosing: boolean;
  chooseFailure: F8Failure | null;
}
export interface LeaseForm {
  scopes: LeaseScope[];
  ttl: number;
}

export interface F8ViewState {
  gate: F8Gate;
  link: LinkInfo | null;
  authority: AuthorityInfo | null;
  notice: Notice;
  sessions: SessionsView | null;
  depth: number;
  reading: boolean;
  readFailure: F8Failure | null;
  selected: ManagedSessionId | null;
  detail: SessionDetail | null;
  tab: F8Tab;
  operation: Operation | null;
  composer: Composer;
  lease: LeaseForm;
  confirming: { kind: 'close' | 'stop' | 'revoke' | 'abandon' } | null;
  draft: string;
  conversation: Conversation | null;
  approval: ApprovalView | null;
  models: ModelsState;
  history: HistoryState;
}

/** How long the runtime may take to come up or to stop before the action is called failed. */
export const RUNTIME_WAIT_MS = 30_000;
const RUNTIME_PACE_MS = 400;
/** How long a binding is asked for again while the authority is still creating it. */
export const BIND_WAIT_MS = 30_000;
const BIND_PACE_MS = 500;
/** The most one watcher waits for a commit before it asks again. */
export const WATCH_MS = 20_000;
/** Files of a change set read without being asked for. The rest are read on request. */
export const FILES_AT_ONCE = 8;
/** Past an approval's deadline, how long until what became of it is read. */
export const EXPIRY_GRACE_MS = 400;
export const EXPIRED_RECHECK_MS = 1000;
/** Reads of a followed turn that may fail in a row before it is called lost. */
const FOLLOW_FAILURES = 20;
export const DEFAULT_SCOPES: readonly LeaseScope[] = [...LEASE_SCOPES];
export const DEFAULT_TTL = LEASE_TTLS[1];

function failureOf(error: unknown): F8Failure {
  return error instanceof F8CallError
    ? error.failure
    : { code: 'internal', retry: false, retry_in_ms: null };
}
function same(
  a: string | null | undefined,
  b: string | null | undefined,
): boolean {
  return (a ?? '').toLowerCase() === (b ?? '').toLowerCase();
}
function fileCard(file: NonNullable<ApprovalDetail['file']>): FileCard {
  return {
    index: file.index,
    path: file.path,
    kind: file.kind,
    move_path: file.move_path,
    bytes: file.bytes,
    text: file.chunk,
    next: file.next,
    reading: false,
    failure: null,
  };
}

interface Intent {
  kind: OperationKind;
  session: ManagedSessionId | null;
  /** The whole action, with the identities it was made with: a retry runs exactly this. */
  run: () => Promise<void>;
}
interface DecisionIntent {
  id: string;
  kind: Decision;
  approval: string;
  revision: number;
  fence: Fence;
}

/**
 * Presentation state of managed runtimes of protocol 2. It decides nothing: it reads what
 * the authority holds, sends one action the user asked for under the identities that action
 * was made with, and reads again. What a session, a runtime, a lease or an approval is comes
 * only from a read; what an action did only from the read that follows it.
 *
 * **Reads.** One reader at a time. It runs while the view is shown, when the authority's
 * durable stream moves (one watcher holds a slot for the next commit) and after every action.
 * Nothing is polled while the view is hidden. A turn is followed by paced reads of its own.
 *
 * **Fences.** Every mutation against a runtime carries the fence of the detail on screen.
 * A refusal as stale is never retried or repaired: the window reads again and says so.
 *
 * **Authority instance.** Every payload names the instance it was read from. When it changes
 * everything runtime-scoped is dropped (followed turn, approval, models), once, with a notice.
 */
export class F8Controller {
  private gate: F8Gate = 'unknown';
  private link: LinkInfo | null = null;
  private authority: AuthorityInfo | null = null;
  private authorityId: string | null = null;
  private notice: Notice = null;
  private sessions: SessionsView | null = null;
  private cursors: (string | null)[] = [null];
  private reading = false;
  private again = false;
  private readFailure: F8Failure | null = null;
  private selected: ManagedSessionId | null = null;
  private detail: SessionDetail | null = null;
  private tab: F8Tab = 'overview';
  private operation: Operation | null = null;
  private intent: Intent | null = null;
  private composer: Composer = {
    mode: 'local_tools',
    workspace: null,
    choosing: false,
    chooseFailure: null,
  };
  private lease: LeaseForm = { scopes: [...DEFAULT_SCOPES], ttl: DEFAULT_TTL };
  private confirming: F8ViewState['confirming'] = null;
  private draft = '';
  private conversation: Conversation | null = null;
  private followToken = 0;
  private approval: ApprovalView | null = null;
  private approvalToken = 0;
  private decisionIntent: DecisionIntent | null = null;
  private expiry: ReturnType<typeof setTimeout> | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private models: ModelsState = { status: 'idle', info: null, failure: null };
  private modelsFor = '';
  private history: HistoryState = {
    status: 'idle',
    page: null,
    failure: null,
    depth: 0,
  };
  private visible = false;
  private watching = false;
  private watchToken = 0;
  private disposed = false;

  constructor(
    private readonly onChange: (state: F8ViewState) => void,
    private readonly client: F8Client = live,
    private readonly mint: () => string = () => crypto.randomUUID(),
    private readonly sleep: (ms: number) => Promise<void> = (ms) =>
      new Promise((resolve) => setTimeout(resolve, ms)),
    private readonly now: () => number = () => Date.now(),
  ) {}

  state(): F8ViewState {
    return {
      gate: this.gate,
      link: this.link,
      authority: this.authority,
      notice: this.notice,
      sessions: this.sessions,
      depth: this.cursors.length - 1,
      reading: this.reading,
      readFailure: this.readFailure,
      selected: this.selected,
      detail: this.detail,
      tab: this.tab,
      operation: this.operation,
      composer: this.composer,
      lease: this.lease,
      confirming: this.confirming,
      draft: this.draft,
      conversation: this.conversation,
      approval: this.approval,
      models: this.models,
      history: this.history,
    };
  }
  private emit() {
    if (!this.disposed) this.onChange(this.state());
  }

  // ------------------------------------------------------------------ lifecycle

  /**
   * Asks once whether the interface is active. Anything but an active answer is final:
   * activation is decided in Rust when the application starts, never by the window.
   */
  async start(): Promise<void> {
    try {
      const state = await this.client.state();
      if (!state.active) {
        this.gate = 'inactive';
        this.emit();
        return;
      }
      this.gate = 'active';
      this.observeState(state);
    } catch {
      this.gate = 'inactive';
      this.emit();
      return;
    }
    this.emit();
    if (this.visible) this.begin();
  }

  /** Nothing is read, watched or polled while the view is hidden. */
  setVisible(visible: boolean): void {
    if (this.visible === visible) return;
    this.visible = visible;
    if (visible) this.begin();
    else this.pause();
  }
  private begin() {
    if (this.gate !== 'active') return;
    void this.refresh();
    void this.watch();
  }
  private pause() {
    this.watchToken++;
    this.watching = false;
    this.clearTimers();
  }
  private clearTimers() {
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }
  dispose(): void {
    this.disposed = true;
    this.followToken++;
    this.approvalToken++;
    this.watchToken++;
    this.clearTimers();
    if (this.expiry) clearTimeout(this.expiry);
    this.expiry = null;
  }

  // ------------------------------------------------------------------ link and authority

  private observeState(state: F8State) {
    this.link = state.link;
    this.authority = state.authority;
    if (state.authority) this.observeAuthority(state.authority.instance_id);
  }
  /**
   * Every payload names the authority instance it came from. A different one than before
   * means every fence, cursor and handle held is stale: they are dropped, once, and the
   * window says so. The session list and the selection are read again, not guessed.
   */
  private observeAuthority(id: string) {
    if (this.authorityId === null) {
      this.authorityId = id;
      return;
    }
    if (same(this.authorityId, id)) return;
    this.authorityId = id;
    this.notice = 'authority_changed';
    this.followToken++;
    this.approvalToken++;
    this.conversation = null;
    this.approval = null;
    this.decisionIntent = null;
    this.models = { status: 'idle', info: null, failure: null };
    this.modelsFor = '';
    this.history = { status: 'idle', page: null, failure: null, depth: 0 };
    this.detail = null;
    this.cursors = [null];
    this.intent = null;
    this.operation = null;
    this.confirming = null;
  }
  dismissNotice(): void {
    this.notice = null;
    this.emit();
  }
  private offline(failure: F8Failure) {
    this.readFailure = failure;
    if (failure.code === 'unavailable' || failure.code === 'incompatible') {
      this.link = {
        state: 'waiting',
        fault:
          failure.code === 'incompatible'
            ? 'incompatible'
            : (this.link?.fault ?? 'lost'),
        retry_in_ms: failure.retry_in_ms,
      };
    }
  }
  /** The user asked to try now, instead of waiting out the delay. */
  async reconnect(): Promise<void> {
    if (this.gate !== 'active') return;
    try {
      this.observeState(await this.client.reconnect());
    } catch (error) {
      this.offline(failureOf(error));
    }
    this.emit();
    await this.refresh();
  }

  // ------------------------------------------------------------------ reads

  /** One reader at a time; a request made while one runs is answered by one more after it. */
  async refresh(): Promise<void> {
    if (this.gate !== 'active' || this.disposed) return;
    if (this.reading) {
      this.again = true;
      return;
    }
    this.reading = true;
    this.emit();
    try {
      const before = this.cursors.at(-1) ?? null;
      const view = await this.client.sessions(before);
      this.observeAuthority(view.authority);
      this.sessions = view;
      this.readFailure = null;
      this.link = { state: 'connected', fault: null, retry_in_ms: null };
      if (this.selected) await this.readDetail(false);
      if (this.tab === 'history' && this.history.status !== 'reading')
        void this.loadHistory(false);
    } catch (error) {
      this.offline(failureOf(error));
      this.scheduleRetry();
    } finally {
      this.reading = false;
      this.emit();
    }
    if (this.again) {
      this.again = false;
      await this.refresh();
    }
  }
  private scheduleRetry() {
    if (!this.visible || this.retryTimer || this.disposed) return;
    const wait = Math.max(this.readFailure?.retry_in_ms ?? 1000, 500);
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      void this.refresh();
    }, wait);
  }

  /** The selected session as the authority holds it now. */
  private async readDetail(emit = true): Promise<SessionDetail | null> {
    const id = this.selected;
    if (!id) return null;
    try {
      const detail = await this.client.session(id);
      if (this.selected !== id) return null;
      this.observeAuthority(detail.authority);
      this.detail = detail;
      this.followRunning(detail);
      this.trackApproval(detail);
      if (this.tab === 'models') void this.loadModels();
      if (emit) this.emit();
      return detail;
    } catch (error) {
      const failure = failureOf(error);
      if (failure.code === 'not_found') this.detail = null;
      this.offline(failure);
      if (emit) this.emit();
      return null;
    }
  }

  /** Waits for the next durable commit and reads again: the one thing that wakes a read. */
  private async watch(): Promise<void> {
    if (this.watching || this.gate !== 'active') return;
    this.watching = true;
    const token = ++this.watchToken;
    let failures = 0;
    let cursor: string | null = null;
    let stream: string | null = null;
    while (token === this.watchToken && !this.disposed) {
      try {
        if (stream === null) {
          const state = await this.client.state();
          this.observeState(state);
          this.emit();
          stream = state.authority?.stream_id ?? null;
          cursor = state.authority?.last_sequence ?? null;
          if (stream === null)
            throw new F8CallError({
              code: 'unavailable',
              retry: true,
              retry_in_ms: null,
            });
        }
        const woke = await this.client.wait(stream, cursor, WATCH_MS);
        if (token !== this.watchToken) break;
        failures = 0;
        this.observeAuthority(woke.authority);
        cursor = woke.head;
        if (woke.changed) await this.refresh();
        else this.emit();
      } catch (error) {
        if (token !== this.watchToken) break;
        const failure = failureOf(error);
        stream = null;
        failures++;
        this.offline(failure);
        this.emit();
        if (failure.code === 'incompatible' || failure.code === 'inactive')
          break;
        await this.sleep(Math.min(failure.retry_in_ms ?? 500 * failures, 5000));
        if (token === this.watchToken) await this.refresh();
      }
    }
    if (token === this.watchToken) this.watching = false;
  }

  // ------------------------------------------------------------------ selection and pages

  select(id: ManagedSessionId | null): void {
    if (same(this.selected, id)) return;
    this.selected = id;
    this.detail = null;
    this.followToken++;
    this.approvalToken++;
    this.conversation = null;
    this.approval = null;
    this.decisionIntent = null;
    this.models = { status: 'idle', info: null, failure: null };
    this.modelsFor = '';
    this.confirming = null;
    this.draft = '';
    this.notice = this.notice === 'authority_changed' ? this.notice : null;
    this.emit();
    if (id) void this.readDetail();
  }
  setTab(tab: F8Tab): void {
    this.tab = tab;
    this.emit();
    if (tab === 'models') void this.loadModels();
    if (tab === 'history') void this.loadHistory(false);
  }
  async older(): Promise<void> {
    const next = this.sessions?.older;
    if (!next) return;
    this.cursors = [...this.cursors, next];
    await this.refresh();
  }
  async newer(): Promise<void> {
    if (this.cursors.length < 2) return;
    this.cursors = this.cursors.slice(0, -1);
    await this.refresh();
  }

  // ------------------------------------------------------------------ actions

  /** Runs one action. A second one waits for the first to finish; none runs twice at once. */
  private async perform(intent: Intent): Promise<void> {
    if (this.operation?.status === 'pending') return;
    this.intent = intent;
    this.notice = this.notice === 'authority_changed' ? this.notice : null;
    await this.execute();
  }
  private async execute(): Promise<void> {
    const intent = this.intent;
    if (!intent) return;
    this.operation = {
      kind: intent.kind,
      session: intent.session,
      status: 'pending',
      step: null,
      failure: null,
    };
    this.emit();
    try {
      await intent.run();
      this.operation = null;
      this.intent = null;
    } catch (error) {
      const failure = failureOf(error);
      if (failure.code === 'stale' || failure.code === 'not_found') {
        // What was shown is not what the authority holds: say so, read again, ask nothing more.
        this.notice = 'stale';
        this.operation = null;
        this.intent = null;
      } else {
        this.operation = {
          kind: intent.kind,
          session: intent.session,
          status: 'failed',
          step: null,
          failure,
        };
      }
    }
    this.emit();
    // The outcome of the action is whatever the next read says it is.
    await this.refresh();
  }
  private step(step: OperationStep) {
    if (this.operation?.status === 'pending') {
      this.operation = { ...this.operation, step };
      this.emit();
    }
  }
  /** Sends the failed action again with the identities it was made with. */
  async retry(): Promise<void> {
    if (this.operation?.status !== 'failed' || !this.operation.failure?.retry)
      return;
    await this.execute();
  }
  dismissOperation(): void {
    if (this.operation?.status !== 'failed') return;
    this.operation = null;
    this.intent = null;
    this.emit();
  }
  confirm(kind: NonNullable<F8ViewState['confirming']>['kind'] | null): void {
    this.confirming = kind ? { kind } : null;
    this.emit();
  }

  async prepare(): Promise<void> {
    const id = this.mint();
    await this.perform({
      kind: 'prepare',
      session: null,
      run: async () => {
        const receipt = await this.client.prepare(id);
        this.observeAuthority(receipt.authority);
        this.cursors = [null];
        this.select(receipt.id);
      },
    });
  }

  async closeSession(): Promise<void> {
    const row = this.detail?.session;
    if (!row) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const revision = row.revision;
    this.confirming = null;
    await this.perform({
      kind: 'close',
      session: row.id,
      run: async () => {
        await this.client.close(id, fence, revision);
      },
    });
  }

  chooseMode(mode: AgentMode): void {
    this.composer = { ...this.composer, mode };
    this.emit();
  }
  /** The native folder dialog. The window is given the one folder the user selects. */
  async chooseFolder(): Promise<void> {
    if (this.composer.choosing) return;
    this.composer = { ...this.composer, choosing: true, chooseFailure: null };
    this.emit();
    try {
      const folder = await this.client.chooseWorkspace();
      this.composer = {
        ...this.composer,
        choosing: false,
        workspace: folder ?? this.composer.workspace,
      };
    } catch (error) {
      this.composer = {
        ...this.composer,
        choosing: false,
        chooseFailure: failureOf(error),
      };
    }
    this.emit();
  }

  async startRuntime(): Promise<void> {
    const row = this.detail?.session;
    if (!row) return;
    const { mode, workspace } = this.composer;
    if (row.runtime.exhausted || (mode === 'local_tools' && workspace === null))
      return;
    const id = this.mint();
    const { revision, generation } = row.runtime;
    const folder = mode === 'local_tools' ? workspace : null;
    await this.perform({
      kind: 'start',
      session: row.id,
      run: async () => {
        await this.client.startRuntime(
          id,
          row.id,
          revision,
          generation,
          mode,
          folder,
        );
        this.step('starting');
        await this.until(
          (detail) => detail.session.runtime.actual === 'ready',
          (detail) =>
            detail.session.runtime.actual === 'failed' ||
            (detail.session.runtime.actual === 'stopped' &&
              detail.session.runtime.desired === 'stopped'),
        );
      },
    });
  }
  async stopRuntime(): Promise<void> {
    const row = this.detail?.session;
    if (!row) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const revision = row.runtime.revision;
    this.confirming = null;
    await this.perform({
      kind: 'stop',
      session: row.id,
      run: async () => {
        await this.client.stopRuntime(id, fence, revision);
        this.step('stopping');
        await this.until(
          (detail) => detail.session.runtime.actual === 'stopped',
          () => false,
        );
      },
    });
  }
  /** Reads the session until `done`, or fails at `failed` or at the bound. */
  private async until(
    done: (detail: SessionDetail) => boolean,
    failed: (detail: SessionDetail) => boolean,
  ): Promise<void> {
    const deadline = this.now() + RUNTIME_WAIT_MS;
    for (;;) {
      const detail = await this.readDetail();
      if (detail && done(detail)) return;
      if (detail && failed(detail))
        throw new F8CallError({
          code: 'runtime_unavailable',
          retry: false,
          retry_in_ms: null,
        });
      if (this.now() >= deadline || this.disposed)
        throw new F8CallError({
          code: 'unavailable',
          retry: true,
          retry_in_ms: null,
        });
      await this.sleep(RUNTIME_PACE_MS);
    }
  }

  toggleScope(scope: LeaseScope): void {
    const scopes = this.lease.scopes.includes(scope)
      ? this.lease.scopes.filter((s) => s !== scope)
      : LEASE_SCOPES.filter(
          (s) => s === scope || this.lease.scopes.includes(s),
        );
    this.lease = { ...this.lease, scopes };
    this.emit();
  }
  setTtl(ttl: number): void {
    if (!(LEASE_TTLS as readonly number[]).includes(ttl)) return;
    this.lease = { ...this.lease, ttl };
    this.emit();
  }
  async grantLease(): Promise<void> {
    const row = this.detail?.session;
    if (!row || this.lease.scopes.length === 0) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const { scopes, ttl } = this.lease;
    const revision = row.revision;
    await this.perform({
      kind: 'grant',
      session: row.id,
      run: async () => {
        await this.client.grantLease(id, fence, revision, scopes, ttl);
      },
    });
  }
  async renewLease(): Promise<void> {
    const row = this.detail?.session;
    const lease = this.detail?.lease;
    if (!row || !lease) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const ttl = this.lease.ttl;
    await this.perform({
      kind: 'renew',
      session: row.id,
      run: async () => {
        await this.client.renewLease(id, fence, lease.id, lease.revision, ttl);
      },
    });
  }
  async revokeLease(): Promise<void> {
    const row = this.detail?.session;
    const lease = this.detail?.lease;
    if (!row || !lease) return;
    const id = this.mint();
    this.confirming = null;
    await this.perform({
      kind: 'revoke',
      session: row.id,
      run: async () => {
        await this.client.revokeLease(id, lease.id, lease.revision);
      },
    });
  }

  /** Binds the folder the user chose. The authority confirms it asynchronously. */
  async bindWorkspace(): Promise<void> {
    const row = this.detail?.session;
    const workspace = this.composer.workspace;
    if (!row || workspace === null) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const revision = row.revision;
    await this.perform({
      kind: 'bind',
      session: row.id,
      run: async () => {
        this.step('binding');
        const deadline = this.now() + BIND_WAIT_MS;
        for (;;) {
          try {
            await this.client.createBinding(id, fence, revision, workspace);
            return;
          } catch (error) {
            const failure = failureOf(error);
            // Still being created: the same intent is asked again, which is the same action.
            if (
              failure.code !== 'busy' ||
              this.now() >= deadline ||
              this.disposed
            )
              throw error;
            await this.sleep(BIND_PACE_MS);
          }
        }
      },
    });
  }
  async resumeBinding(): Promise<void> {
    const row = this.detail?.session;
    if (!row) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const revision = row.revision;
    await this.perform({
      kind: 'resume',
      session: row.id,
      run: async () => {
        await this.client.resumeBinding(id, fence, revision);
      },
    });
  }
  async abandonBinding(): Promise<void> {
    const row = this.detail?.session;
    const operation = this.detail?.binding.operation;
    if (!row || !operation) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    this.confirming = null;
    await this.perform({
      kind: 'abandon',
      session: row.id,
      run: async () => {
        await this.client.abandonBinding(id, fence, operation);
      },
    });
  }

  // ------------------------------------------------------------------ conversation

  setDraft(text: string): void {
    this.draft = text;
    this.emit();
  }
  async startTurn(): Promise<void> {
    const row = this.detail?.session;
    const prompt = this.draft;
    if (!row || prompt.trim().length === 0) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const revision = row.revision;
    await this.perform({
      kind: 'turn',
      session: row.id,
      run: async () => {
        const receipt = await this.client.startTurn(
          id,
          fence,
          revision,
          prompt,
        );
        // The authority admitted it: the text is its, not a draft any more.
        this.draft = '';
        this.tab = 'conversation';
        void this.follow(receipt.id);
      },
    });
  }
  async cancelTurn(): Promise<void> {
    const row = this.detail?.session;
    const info = this.conversation?.info;
    if (!row || !info || turnEnded(info.turn.state)) return;
    const id = this.mint();
    const fence = row.runtime.fence;
    const turn = info.turn.id;
    const revision = info.turn.revision;
    await this.perform({
      kind: 'cancel',
      session: row.id,
      run: async () => {
        await this.client.cancelTurn(id, fence, turn, revision);
      },
    });
  }
  /** Opens a turn the session has: its stream is shown as far as the authority kept it. */
  openTurn(turn: string): void {
    if (this.conversation && same(this.conversation.turn, turn)) return;
    this.tab = 'conversation';
    void this.follow(turn, true);
  }

  /** A turn that is running is followed whatever started it, so a restart of the window resumes it. */
  private followRunning(detail: SessionDetail) {
    const newest = detail.turns[0];
    if (!newest || turnEnded(newest.turn.state)) return;
    const open = this.conversation;
    if (open && same(open.turn, newest.turn.id)) {
      if (open.following || open.lost) return;
    } else if (open && (open.explicit || open.following)) {
      // The user is reading another turn, or a follower is already reading one.
      return;
    }
    void this.follow(newest.turn.id);
  }

  /** Paced reads of one turn until it is seen to end, or can no longer be followed. */
  private async follow(turn: string, explicit = false): Promise<void> {
    const token = ++this.followToken;
    this.conversation = {
      turn,
      explicit,
      info: null,
      transcript: emptyTranscript,
      cursor: null,
      following: true,
      reconnecting: false,
      lost: false,
      gap: false,
      approval: null,
    };
    this.emit();
    let failures = 0;
    while (token === this.followToken && !this.disposed) {
      try {
        const c: Conversation | null = this.conversation;
        if (!c) return;
        const reading = await this.client.readTurn(turn, c.cursor, true);
        if (token !== this.followToken) return;
        failures = 0;
        this.observeAuthority(reading.authority);
        if (token !== this.followToken) {
          // The authority changed under this turn: what was dropped is shown as dropped.
          this.emit();
          return;
        }
        const done = turnEnded(reading.turn.turn.state) && !reading.more;
        this.conversation = {
          ...c,
          info: reading.turn,
          transcript: appendEvents(c.transcript, reading.events),
          cursor: reading.next ?? c.cursor,
          following: !done,
          reconnecting: false,
          gap: c.gap || reading.gap,
          approval: reading.approval,
        };
        this.emit();
        if (reading.approval) void this.loadApproval(reading.approval.id);
        if (done) break;
      } catch (error) {
        if (token !== this.followToken) return;
        const failure = failureOf(error);
        failures++;
        if (
          failure.code === 'not_found' ||
          failure.code === 'incompatible' ||
          failure.code === 'inactive' ||
          failures >= FOLLOW_FAILURES
        ) {
          if (this.conversation)
            this.conversation = {
              ...this.conversation,
              following: false,
              reconnecting: false,
              lost: true,
            };
          this.emit();
          break;
        }
        if (this.conversation)
          this.conversation = { ...this.conversation, reconnecting: true };
        this.emit();
        await this.sleep(Math.min(500 * failures, 5000));
      }
    }
    if (token === this.followToken) await this.refresh();
  }

  // ------------------------------------------------------------------ approvals

  /** Reads the approval a pending turn is waiting on, once per approval. */
  private trackApproval(detail: SessionDetail) {
    const pending = detail.approval;
    if (!pending) {
      // A pending approval that is no longer there ended: what is shown is read again once.
      if (
        this.approval &&
        this.approval.detail.approval.state === 'pending' &&
        !this.approval.decision
      )
        void this.loadApproval(this.approval.detail.approval.id, true);
      return;
    }
    if (
      this.approval &&
      same(this.approval.detail.approval.id, pending.approval.id)
    )
      return;
    void this.loadApproval(pending.approval.id);
  }

  private async loadApproval(id: string, force = false): Promise<void> {
    // The same approval is read again only when something says it changed.
    if (!force && this.approval && same(this.approval.detail.approval.id, id))
      return;
    const token = ++this.approvalToken;
    try {
      const detail = await this.client.readApproval(id, null, null);
      if (token !== this.approvalToken) return;
      const kept =
        this.approval && same(this.approval.detail.approval.id, id)
          ? this.approval
          : null;
      this.approval = {
        detail,
        files: detail.file ? [fileCard(detail.file)] : [],
        listing: false,
        decision: kept?.decision ?? null,
      };
      this.emit();
      this.armExpiry(detail);
      if (detail.files !== null && detail.files > 1)
        await this.readFiles(id, token);
    } catch (error) {
      if (token !== this.approvalToken) return;
      const failure = failureOf(error);
      if (failure.code === 'not_found') {
        this.approval = null;
        this.emit();
      }
    }
  }
  private armExpiry(detail: ApprovalDetail) {
    if (this.expiry) clearTimeout(this.expiry);
    this.expiry = null;
    if (detail.approval.state !== 'pending') return;
    const wait =
      Date.parse(detail.approval.expires_at) - this.now() + EXPIRY_GRACE_MS;
    this.expiry = setTimeout(
      () => {
        this.expiry = null;
        void this.loadApproval(detail.approval.id, true);
        void this.refresh();
      },
      // An approval the authority still calls pending past its end is looked at again in a
      // second, not in a loop.
      wait > 0 ? wait : EXPIRED_RECHECK_MS,
    );
  }
  private async readFiles(id: string, token: number): Promise<void> {
    const approval = this.approval;
    if (!approval || approval.detail.files === null) return;
    const total = approval.detail.files;
    const upto = Math.min(total, FILES_AT_ONCE);
    for (let index = 1; index < upto; index++) {
      try {
        const detail = await this.client.readApproval(id, index, null);
        if (token !== this.approvalToken || !this.approval) return;
        // The file that was asked for, once: anything else is not this approval's change set.
        if (
          detail.file?.index === index &&
          !this.approval.files.some((f) => f.index === index)
        )
          this.approval = {
            ...this.approval,
            files: [...this.approval.files, fileCard(detail.file)],
          };
        this.emit();
      } catch {
        return;
      }
    }
  }
  /** The rest of a file's diff, one page at a time. */
  async moreOfFile(index: number): Promise<void> {
    const approval = this.approval;
    const file = approval?.files.find((f) => f.index === index);
    if (!approval || !file || file.next === null || file.reading) return;
    const id = approval.detail.approval.id;
    const patch = (change: Partial<FileCard>) => {
      if (!this.approval) return;
      this.approval = {
        ...this.approval,
        files: this.approval.files.map((f) =>
          f.index === index ? { ...f, ...change } : f,
        ),
      };
      this.emit();
    };
    patch({ reading: true, failure: null });
    try {
      const detail = await this.client.readApproval(id, index, file.next);
      const page = detail.file;
      if (!page || page.index !== index || page.offset !== file.next)
        throw new F8CallError({
          code: 'internal',
          retry: false,
          retry_in_ms: null,
        });
      patch({ text: file.text + page.chunk, next: page.next, reading: false });
    } catch (error) {
      patch({ reading: false, failure: failureOf(error) });
    }
  }
  async moreFiles(): Promise<void> {
    const approval = this.approval;
    if (!approval || approval.detail.files === null || approval.listing) return;
    const total = approval.detail.files;
    const id = approval.detail.approval.id;
    const token = this.approvalToken;
    this.approval = { ...approval, listing: true };
    this.emit();
    const have = new Set(approval.files.map((f) => f.index));
    for (let index = 0; index < total && have.size < total; index++) {
      if (have.has(index)) continue;
      try {
        const detail = await this.client.readApproval(id, index, null);
        if (token !== this.approvalToken || !this.approval) return;
        if (detail.file?.index === index && !have.has(index)) {
          have.add(index);
          this.approval = {
            ...this.approval,
            files: [...this.approval.files, fileCard(detail.file)],
          };
        }
      } catch {
        break;
      }
    }
    if (this.approval) this.approval = { ...this.approval, listing: false };
    this.emit();
  }

  /** One decision, as the user's one action: its identity is made once. */
  async decide(kind: Decision): Promise<void> {
    const approval = this.approval;
    const row = this.detail?.session;
    if (
      !approval ||
      !row ||
      approval.decision?.status === 'pending' ||
      !approval.detail.decisions.includes(kind)
    )
      return;
    const fence = row.runtime.fence;
    if (!approval.detail.approval.decidable || fence.instance === null) return;
    this.decisionIntent = {
      id: this.mint(),
      kind,
      approval: approval.detail.approval.id,
      revision: approval.detail.approval.revision,
      fence,
    };
    await this.sendDecision();
  }
  async retryDecision(): Promise<void> {
    if (
      this.approval?.decision?.status !== 'failed' ||
      !this.approval.decision.failure?.retry
    )
      return;
    await this.sendDecision();
  }
  dismissDecision(): void {
    if (!this.approval?.decision) return;
    this.approval = { ...this.approval, decision: null };
    this.decisionIntent = null;
    this.emit();
  }
  private async sendDecision(): Promise<void> {
    const intent = this.decisionIntent;
    if (!intent || !this.approval) return;
    this.approval = {
      ...this.approval,
      decision: { kind: intent.kind, status: 'pending', failure: null },
    };
    this.emit();
    try {
      await this.client.decide(
        intent.kind,
        intent.id,
        intent.fence,
        intent.approval,
        intent.revision,
      );
      this.decisionIntent = null;
      if (this.approval) this.approval = { ...this.approval, decision: null };
    } catch (error) {
      const failure = failureOf(error);
      if (failure.code === 'stale') this.notice = 'stale';
      if (this.approval)
        this.approval = {
          ...this.approval,
          decision: { kind: intent.kind, status: 'failed', failure },
        };
      if (!failure.retry) this.decisionIntent = null;
    }
    this.emit();
    // What the decision did is what the approval and the turn say now.
    await this.loadApproval(intent.approval, true);
    await this.readDetail();
  }

  // ------------------------------------------------------------------ models and history

  async loadModels(): Promise<void> {
    const runtime = this.detail?.session.runtime;
    if (
      !runtime ||
      runtime.actual !== 'ready' ||
      runtime.fence.instance === null
    ) {
      this.models = { status: 'idle', info: null, failure: null };
      this.modelsFor = '';
      this.emit();
      return;
    }
    const key = `${runtime.fence.generation}:${runtime.fence.instance}`;
    if (this.modelsFor === key && this.models.status !== 'failed') return;
    this.modelsFor = key;
    this.models = { status: 'reading', info: null, failure: null };
    this.emit();
    try {
      const info = await this.client.models(runtime.fence);
      if (this.modelsFor !== key) return;
      this.observeAuthority(info.authority);
      this.models = { status: 'ready', info, failure: null };
    } catch (error) {
      if (this.modelsFor !== key) return;
      this.models = { status: 'failed', info: null, failure: failureOf(error) };
    }
    this.emit();
  }

  /** The newest window of the durable history, or one further back. */
  async loadHistory(older: boolean): Promise<void> {
    const page = this.history.page;
    if (older && !(page?.page.older && page.page.stream)) return;
    this.history = { ...this.history, status: 'reading', failure: null };
    this.emit();
    try {
      const info = await this.client.audit(
        older && page?.page.older
          ? { stream_id: page.page.stream, before: page.page.older }
          : null,
      );
      this.observeAuthority(info.authority);
      this.history = {
        status: 'ready',
        page: info,
        failure: null,
        depth: older ? this.history.depth + 1 : 0,
      };
    } catch (error) {
      const failure = failureOf(error);
      // History moved under the position: the newest window is read instead.
      this.history = { ...this.history, status: 'failed', failure };
      if (failure.code === 'stale' && older) {
        this.history = { status: 'idle', page: null, failure: null, depth: 0 };
        this.emit();
        await this.loadHistory(false);
        return;
      }
    }
    this.emit();
  }
  async newestHistory(): Promise<void> {
    this.history = { status: 'idle', page: null, failure: null, depth: 0 };
    await this.loadHistory(false);
  }

  /** The session shown, as a row. */
  get row(): SessionRow | null {
    return this.detail?.session ?? null;
  }
}
