import type {
  Applied,
  ConfigurationFailure,
  History,
  NewDocumentKind,
  Operation,
  Outcome,
  ParkedRecord,
  Plan,
} from '../../contracts/configuration';
import {
  ConfigurationCallError,
  applyPlan,
  authorHook,
  authorMcp,
  editHook,
  createDocument,
  editDocument,
  exportDocument,
  importDocument,
  editMcp,
  deleteRecord,
  exportRecord,
  importRecord,
  planChange,
  planUndo,
  readHistory,
  readRecords,
} from '../../desktop/configuration-client';

export interface ChangesClient {
  plan(operation: Operation, target: string): Promise<Plan>;
  apply(plan: string): Promise<Applied>;
  history(cursor: string | null): Promise<History>;
  undo(change: string): Promise<Plan>;
  authorHook(
    event: string,
    matcher: string | null,
    timeout: number | null,
  ): Promise<Plan>;
  editHook(item: string): Promise<Plan>;
  editDocument(item: string): Promise<Plan>;
  createDocument(kind: NewDocumentKind): Promise<Plan>;
  exportDocument(item: string): Promise<boolean>;
  importDocument(item: string): Promise<Plan>;
  authorMcp(
    name: string,
    kind: 'local' | 'remote',
    agent?: 'opencode' | 'codex',
  ): Promise<Plan>;
  editMcp(item: string): Promise<Plan>;
  records(): Promise<ParkedRecord[]>;
  deleteRecord(record: string): Promise<boolean>;
  exportRecord(record: string): Promise<boolean>;
  importRecord(): Promise<boolean>;
}
const desktopClient: ChangesClient = {
  plan: planChange,
  apply: applyPlan,
  history: readHistory,
  undo: planUndo,
  authorHook,
  editHook,
  editDocument,
  createDocument,
  exportDocument,
  importDocument,
  authorMcp,
  editMcp,
  records: readRecords,
  deleteRecord,
  exportRecord,
  importRecord,
};

export type Panel = 'items' | 'history' | 'parked';

export interface ChangesState {
  panel: Panel;
  /** The change the user is looking at before confirming it. */
  plan: Plan | null;
  planning: boolean;
  /** Waiting for the apply, and for the native window when the risk needs one. */
  applying: boolean;
  /** How the last apply ended. Cleared by the next plan. */
  result: Outcome | null;
  failure: ConfigurationFailure | null;
  history: History | null;
  records: ParkedRecord[] | null;
  /** Something else in flight: a deletion, an export, an import. */
  busy: boolean;
  /** What the last record operation came to. */
  notice: 'exported' | 'imported' | 'not_done' | 'expired' | null;
}

/**
 * The one owner of the changes in the window. It asks for a plan by naming an item, a
 * record or a change the desktop gave it; it holds the redacted view of the plan and
 * nothing of the file. A plan is applied by its identifier, once. A change that needs the
 * native window waits for it here, and never answers it.
 */
export class ChangesController {
  private panel: Panel = 'items';
  private plan: Plan | null = null;
  private planning = false;
  private applying = false;
  private result: Outcome | null = null;
  private failure: ConfigurationFailure | null = null;
  private history: History | null = null;
  private records: ParkedRecord[] | null = null;
  private busy = false;
  private notice: ChangesState['notice'] = null;
  private disposed = false;
  private expiry: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly publish: (state: ChangesState) => void,
    /** Called when the file may have changed: the inventory is read again. */
    private readonly changed: () => void = () => undefined,
    private readonly client: ChangesClient = desktopClient,
  ) {}

  state(): ChangesState {
    return {
      panel: this.panel,
      plan: this.plan,
      planning: this.planning,
      applying: this.applying,
      result: this.result,
      failure: this.failure,
      history: this.history,
      records: this.records,
      busy: this.busy,
      notice: this.notice,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }
  private fail(error: unknown): ConfigurationFailure {
    return error instanceof ConfigurationCallError ? error.code : 'internal';
  }

  show(panel: Panel) {
    if (this.disposed || panel === this.panel) return;
    this.panel = panel;
    this.notice = null;
    this.emit();
    if (panel === 'history' && this.history === null) this.loadHistory(false);
    if (panel === 'parked') this.loadRecords();
  }

  /** Drafts a change. Nothing is written: a plan is a view of what would be. */
  draft(operation: Operation, target: string) {
    if (this.disposed || this.planning || this.applying) return;
    this.planning = true;
    this.failure = null;
    this.result = null;
    this.notice = null;
    this.emit();
    void this.client.plan(operation, target).then(
      (plan) => this.hold(plan),
      (error: unknown) => {
        this.planning = false;
        this.failure = this.fail(error);
        this.emit();
      },
    );
  }

  /**
   * Something the user writes. The window asks for the text in a window of the desktop's own
   * and answers with the plan; closing that window is a cancel, not a failure.
   */
  author(make: (client: ChangesClient) => Promise<Plan>) {
    if (this.disposed || this.planning || this.applying) return;
    this.planning = true;
    this.failure = null;
    this.result = null;
    this.notice = null;
    this.emit();
    void make(this.client).then(
      (plan) => this.hold(plan),
      (error: unknown) => {
        this.planning = false;
        const failure = this.fail(error);
        if (failure === 'cancelled') this.notice = 'not_done';
        else this.failure = failure;
        this.emit();
      },
    );
  }

  /** The change that undoes one in the history, as a plan to confirm. */
  undo(change: string) {
    if (this.disposed || this.planning || this.applying) return;
    this.planning = true;
    this.failure = null;
    this.result = null;
    this.emit();
    void this.client.undo(change).then(
      (plan) => this.hold(plan),
      (error: unknown) => {
        this.planning = false;
        this.failure = this.fail(error);
        this.emit();
      },
    );
  }

  private hold(plan: Plan) {
    this.planning = false;
    if (this.disposed) return;
    this.plan = plan;
    this.clearExpiry();
    // The desktop forgets a plan after its lifetime; the window stops offering it then.
    this.expiry = setTimeout(
      () => {
        if (this.plan?.id === plan.id && !this.applying) {
          this.plan = null;
          this.notice = 'expired';
          this.emit();
        }
      },
      Math.max(plan.expires_in_seconds, 1) * 1000,
    );
    this.emit();
  }
  private clearExpiry() {
    if (this.expiry !== null) clearTimeout(this.expiry);
    this.expiry = null;
  }

  cancel() {
    if (this.disposed || this.applying) return;
    this.clearExpiry();
    this.plan = null;
    this.emit();
  }

  /** The user's confirmation in the page. A risk above normal then asks in the native window. */
  apply() {
    const plan = this.plan;
    if (this.disposed || plan === null || this.applying) return;
    this.applying = true;
    this.failure = null;
    this.clearExpiry();
    this.emit();
    void this.client.apply(plan.id).then(
      (applied) => {
        this.applying = false;
        this.plan = null;
        this.result = applied.outcome;
        // Whatever it came to, what the file holds is read again, and the lists with it.
        this.history = null;
        this.records = null;
        this.emit();
        this.changed();
        if (this.panel === 'history') this.loadHistory(false);
        if (this.panel === 'parked') this.loadRecords();
      },
      (error: unknown) => {
        this.applying = false;
        this.plan = null;
        this.failure = this.fail(error);
        this.emit();
        this.changed();
      },
    );
  }

  /** The first page, or the next. */
  loadHistory(more: boolean) {
    if (this.disposed) return;
    const cursor = more ? (this.history?.next ?? null) : null;
    if (more && cursor === null) return;
    void this.client.history(cursor).then(
      (page) => {
        if (this.disposed) return;
        this.history = {
          changes: more
            ? [...(this.history?.changes ?? []), ...page.changes]
            : page.changes,
          next: page.next,
        };
        this.emit();
      },
      (error: unknown) => {
        this.failure = this.fail(error);
        this.emit();
      },
    );
  }

  loadRecords() {
    if (this.disposed) return;
    void this.client.records().then(
      (records) => {
        if (this.disposed) return;
        this.records = records;
        this.emit();
      },
      (error: unknown) => {
        this.failure = this.fail(error);
        this.emit();
      },
    );
  }

  private record(
    work: () => Promise<boolean>,
    done: 'exported' | 'imported' | null,
  ) {
    if (this.disposed || this.busy) return;
    this.busy = true;
    this.failure = null;
    this.result = null;
    this.notice = null;
    this.emit();
    void work().then(
      (finished) => {
        this.busy = false;
        this.notice = finished ? done : 'not_done';
        this.emit();
        this.loadRecords();
      },
      (error: unknown) => {
        this.busy = false;
        this.failure = this.fail(error);
        this.emit();
      },
    );
  }
  deleteRecord(record: string) {
    this.record(() => this.client.deleteRecord(record), null);
  }
  exportRecord(record: string) {
    this.record(() => this.client.exportRecord(record), 'exported');
  }
  exportDocument(item: string) {
    this.record(() => this.client.exportDocument(item), 'exported');
  }
  importRecord() {
    this.record(() => this.client.importRecord(), 'imported');
  }

  dispose() {
    this.disposed = true;
    this.clearExpiry();
  }
}
