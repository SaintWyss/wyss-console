/**
 * The project-centered workspace (M1) as the window holds it.
 *
 * The window never builds an effect: it builds a closed envelope naming what it was looking at
 * (the host, the project, the workspace, the binding and its revision, and the revision of its
 * own selection) and the desktop checks every relationship before it writes anything. A launch
 * that came back unknown stays unknown here too: nothing sends it again by itself.
 */
import type {
  Command,
  Context,
  Envelope,
  Failure,
  LaunchChoice,
  MigrationSummary,
  Overview,
  PickView,
  Preferences,
  ProjectView,
  Receipt,
  RollbackReport,
  SessionView,
  WorkspaceView,
} from '../../contracts/projects';
import {
  ProjectsCallError,
  desktopProjects,
  type ProjectsClient,
} from '../../desktop/projects-client';
import { split, remove, resize, type Direction } from '../terminal/layout';

export type Phase = 'idle' | 'loading' | 'ready' | 'failed';

export interface Notice {
  kind: 'error' | 'info';
  text: string;
  failure: Failure | null;
}

export interface ProjectsState {
  phase: Phase;
  overview: Overview | null;
  projects: ProjectView[];
  /** The project in view; null is Scratch. */
  project: string | null;
  workspaceId: string | null;
  workspace: WorkspaceView | null;
  /** Every session the store knows for the project in view (or the unassigned ones). */
  sessions: SessionView[];
  /** Unassigned sessions, for adopting into a project. */
  unassigned: SessionView[];
  preview: MigrationSummary | null;
  rollback: RollbackReport | null;
  notice: Notice | null;
  busy: boolean;
  /**
   * The session this window itself just started. Its pane takes input at once: a new shell
   * asks the terminal questions that only the window holding input may answer, and nobody else
   * can be typing into a terminal that was made a moment ago.
   */
  launched: string | null;
  /** The revision the window has read; a change elsewhere moves it. */
  revision: string | null;
}

export interface Timings {
  poll: number;
}

const WORDS: Record<string, string> = {
  revision_conflict:
    'Changed somewhere else. The view was refreshed; try again.',
  context_stale:
    'That no longer matches what is shown. The view was refreshed; nothing was changed.',
  leaf_busy: 'This pane is busy with a launch.',
  root_missing: 'The folder is missing.',
  root_identity_changed: 'The folder was replaced by another one.',
  root_ineligible: 'That folder cannot be used.',
  host_offline: 'No terminal host answers. Nothing was started.',
  host_changed:
    'The terminal host is not the one this session lived in. Nothing was sent.',
  unsupported_capability: 'Not available in this version or for this choice.',
  profile_unavailable: 'That profile is no longer available.',
  owner_refused: 'The owner of that session refused. Nothing was changed.',
  effect_ambiguous:
    'The outcome is unknown. Nothing was started again; resolve it below.',
  io_failure: 'The store could not be written.',
  capacity_exceeded: 'A limit was reached. Nothing was changed.',
  invalid_record: 'That change is not valid.',
  unsupported_schema: 'The store is from a newer version and is read-only.',
};

export function noticeOf(failure: Failure): Notice {
  return {
    kind: 'error',
    text: WORDS[failure.code] ?? 'Not done.',
    failure,
  };
}

const initial = (): ProjectsState => ({
  phase: 'idle',
  overview: null,
  projects: [],
  project: null,
  workspaceId: null,
  workspace: null,
  sessions: [],
  unassigned: [],
  preview: null,
  rollback: null,
  notice: null,
  busy: false,
  launched: null,
  revision: null,
});

export class ProjectsController {
  private s: ProjectsState = initial();
  private selection = 1;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private visible = false;
  private disposed = false;
  private loading = false;
  private again = false;

  constructor(
    private readonly emit: (next: ProjectsState) => void,
    private readonly client: ProjectsClient = desktopProjects,
    private readonly timings: Timings = { poll: 2500 },
    private readonly newId: () => string = () => crypto.randomUUID(),
  ) {}

  state(): ProjectsState {
    return this.s;
  }

  private set(change: Partial<ProjectsState>) {
    if (this.disposed) return;
    this.s = { ...this.s, ...change };
    this.emit(this.s);
  }

  private ready() {
    const store = this.s.overview?.store;
    return store?.state === 'ready' ? store : null;
  }

  /** One timer for the whole view, only while it is shown: no loop per workspace. */
  setVisible(visible: boolean) {
    if (this.visible === visible) return;
    this.visible = visible;
    if (visible) {
      void this.refresh();
      this.schedule();
    } else if (this.timer !== undefined) {
      clearTimeout(this.timer);
      this.timer = undefined;
    }
  }

  private schedule() {
    if (this.disposed || !this.visible) return;
    this.timer = setTimeout(() => {
      void this.tick().finally(() => this.schedule());
    }, this.timings.poll);
  }

  /** A cheap read of the revision; everything is read again only when it moved. */
  private async tick() {
    if (this.s.busy || this.loading) return;
    try {
      const overview = await this.client.overview();
      const was = this.ready()?.revision;
      const now =
        overview.store.state === 'ready' ? overview.store.revision : null;
      const hostMoved =
        overview.host.epoch !== (this.s.overview?.host.epoch ?? null);
      this.set({ overview });
      if (
        overview.enabled !== (this.s.overview?.enabled ?? false) ||
        was !== now ||
        hostMoved
      )
        await this.refresh();
    } catch {
      // A failed poll is silent; the next one asks again.
    }
  }

  dispose() {
    this.disposed = true;
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }

  /** Reads the overview and, when there is a store, everything the view shows. */
  async refresh(): Promise<void> {
    if (this.loading) {
      this.again = true;
      return;
    }
    this.loading = true;
    try {
      do {
        this.again = false;
        await this.readAll();
      } while (this.again && !this.disposed);
    } finally {
      this.loading = false;
    }
  }

  private async readAll() {
    if (this.s.phase === 'idle') this.set({ phase: 'loading' });
    try {
      const overview = await this.client.overview();
      const store = overview.store;
      if (!overview.enabled || store.state !== 'ready') {
        this.set({
          overview,
          phase: 'ready',
          projects: [],
          workspace: null,
          sessions: [],
          unassigned: [],
          revision: null,
        });
        return;
      }
      const projects = await this.allProjects();
      let project = this.s.project;
      if (project && !projects.some((p) => p.id === project && !p.tombstone))
        project = null;
      const workspaceId = this.pickWorkspace(
        project,
        projects,
        store.scratchWorkspaceId,
      );
      const [workspace, sessions, unassigned] = await Promise.all([
        this.client.workspace(workspaceId),
        this.allSessions(project),
        this.allSessions(null),
      ]);
      this.set({
        overview,
        phase: 'ready',
        projects,
        project,
        workspaceId,
        workspace,
        sessions,
        unassigned,
        revision: store.revision,
      });
    } catch {
      // A read that failed is not shown as an alert: nothing was asked for. The view says it
      // could not read, and the next poll asks again.
      this.set({ phase: 'failed' });
    }
  }

  private pickWorkspace(
    project: string | null,
    projects: ProjectView[],
    scratch: string,
  ): string {
    if (project === null) return scratch;
    const found = projects.find((p) => p.id === project);
    const own = this.s.workspaceId;
    if (own && found?.workspaceIds.includes(own)) return own;
    return found?.defaultWorkspaceId ?? found?.workspaceIds[0] ?? scratch;
  }

  private async allProjects(): Promise<ProjectView[]> {
    const rows: ProjectView[] = [];
    let cursor: string | null = null;
    for (let page = 0; page < 8; page++) {
      const answer = await this.client.projects(cursor);
      rows.push(...answer.items);
      cursor = answer.cursor;
      if (!cursor) break;
    }
    return rows;
  }

  private async allSessions(project: string | null): Promise<SessionView[]> {
    const rows: SessionView[] = [];
    let cursor: string | null = null;
    for (let page = 0; page < 8; page++) {
      const answer = await this.client.sessions(project, cursor);
      rows.push(...answer.items);
      cursor = answer.cursor;
      if (!cursor) break;
    }
    return rows;
  }

  // ---- selecting ----------------------------------------------------------------------------

  /** Shows a project (or Scratch). Presentation only: nothing is launched, stopped or resumed. */
  async selectProject(project: string | null) {
    this.selection += 1;
    await this.client.forgetPicks().catch(() => undefined);
    this.set({ project, workspaceId: null, notice: null, launched: null });
    await this.refresh();
  }

  async selectWorkspace(id: string) {
    this.selection += 1;
    await this.client.forgetPicks().catch(() => undefined);
    this.set({ workspaceId: id, notice: null, launched: null });
    await this.refresh();
  }

  dismiss() {
    this.set({ notice: null });
  }

  // ---- the context a command is sent in -------------------------------------------------------

  private context(): Context | null {
    const store = this.ready();
    if (!store) return null;
    const project =
      this.s.projects.find((p) => p.id === this.s.project) ?? null;
    return {
      hostId: store.hostId,
      projectId: this.s.project,
      workspaceId: this.s.workspaceId,
      bindingId: project?.root?.bindingId ?? null,
      bindingRevision: project?.root?.bindingRevision ?? null,
      selectionRevision: String(this.selection),
    };
  }

  /** Sends one command and refreshes. A refusal is shown as it is; nothing is replayed. */
  async send(
    command: Command,
    override?: Partial<Context>,
    onReceipt?: (receipt: Receipt) => void,
  ): Promise<Receipt | Failure> {
    const store = this.ready();
    const context = this.context();
    if (!store || !context) return this.refuse('invalid_record');
    const envelope: Envelope = {
      schema: 1,
      operationId: this.newId(),
      expectedStoreRevision: store.revision,
      context: { ...context, ...override },
      command,
    };
    return this.dispatch(envelope, onReceipt);
  }

  private refuse(code: Failure['code']): Failure {
    const failure: Failure = {
      code,
      effect: 'none',
      nextAction: 'none',
      operationId: null,
      context: null,
      field: null,
    };
    this.set({ notice: noticeOf(failure) });
    return failure;
  }

  private async dispatch(
    envelope: Envelope,
    onReceipt?: (receipt: Receipt) => void,
  ): Promise<Receipt | Failure> {
    if (this.s.busy) return this.refuse('leaf_busy');
    this.set({ busy: true, notice: null });
    try {
      const receipt = await this.client.execute(envelope);
      this.set({ busy: false });
      onReceipt?.(receipt);
      await this.refresh();
      return receipt;
    } catch (error) {
      const failure =
        error instanceof ProjectsCallError
          ? error.failure
          : this.refuse('io_failure');
      this.set({ busy: false, notice: noticeOf(failure) });
      // What the window knew is stale: read again, and let the user decide again.
      await this.refresh();
      return failure;
    }
  }

  // ---- the commands ---------------------------------------------------------------------------

  /** The native dialog; cancelling returns nothing and changes nothing. */
  async choose(
    purpose: 'register' | 'parent' | 'relink',
  ): Promise<PickView | null> {
    try {
      return await this.client.chooseFolder(purpose, String(this.selection));
    } catch (error) {
      if (error instanceof ProjectsCallError)
        this.set({ notice: noticeOf(error.failure) });
      return null;
    }
  }

  async registerFolder() {
    const pick = await this.choose('register');
    if (!pick) return;
    const done = await this.send({
      kind: 'registerExisting',
      selectionToken: pick.token,
    });
    if ('created' in done) {
      const project = done.created.find((c) => c.role === 'project');
      if (project) await this.selectProject(project.id);
    }
  }

  async createFolder(name: string) {
    const pick = await this.choose('parent');
    if (!pick) return;
    const done = await this.send({
      kind: 'createFolder',
      selectionToken: pick.token,
      name,
    });
    if ('created' in done) {
      const project = done.created.find((c) => c.role === 'project');
      if (project) await this.selectProject(project.id);
    }
  }

  async relink(projectId: string, oldBindingId: string) {
    const pick = await this.choose('relink');
    if (!pick) return;
    await this.send({
      kind: 'relink',
      projectId,
      oldBindingId,
      selectionToken: pick.token,
    });
  }

  rename(projectId: string, label: string) {
    return this.send({ kind: 'renameProject', projectId, label });
  }
  archive(projectId: string, archived: boolean) {
    return this.send({ kind: 'archiveProject', projectId, archived });
  }
  async remove(projectId: string, retainHistory: boolean) {
    const done = await this.send({
      kind: 'removeProject',
      projectId,
      retainHistory,
    });
    if ('created' in done && this.s.project === projectId)
      await this.selectProject(null);
  }
  createWorkspace(projectId: string, label: string) {
    return this.send({ kind: 'createWorkspace', projectId, label });
  }
  duplicateWorkspace(sourceWorkspaceId: string, label: string) {
    return this.send({ kind: 'duplicateWorkspace', sourceWorkspaceId, label });
  }
  deleteWorkspace(workspaceId: string) {
    return this.send({ kind: 'deleteWorkspace', workspaceId });
  }
  closeWorkspace(workspaceId: string) {
    return this.send({ kind: 'closeWorkspace', workspaceId });
  }
  adopt(associationId: string, projectId: string, bindingId: string) {
    return this.send({
      kind: 'adoptScratch',
      associationId,
      projectId,
      bindingId,
    });
  }
  attach(leafId: string, associationId: string) {
    const workspaceId = this.s.workspaceId;
    if (!workspaceId) return this.refuse('context_stale');
    return this.send({ kind: 'attach', workspaceId, leafId, associationId });
  }
  detach(leafId: string, associationId: string) {
    const workspaceId = this.s.workspaceId;
    if (!workspaceId) return this.refuse('context_stale');
    return this.send({ kind: 'detach', workspaceId, leafId, associationId });
  }

  /** An explicit launch into an empty pane. Nothing is chosen for the user. */
  launch(choice: Omit<LaunchChoice, 'workspaceId'>) {
    const workspaceId = this.s.workspaceId;
    if (!workspaceId) return this.refuse('context_stale');
    return this.send(
      { kind: 'launch', workspaceId, ...choice },
      undefined,
      (receipt) => {
        const made = receipt.created.find((c) => c.role === 'association');
        if (made) this.set({ launched: made.id });
      },
    );
  }

  reconcile(
    launchOperationId: string,
    resolution: Extract<Command, { kind: 'reconcileLaunch' }>['resolution'],
  ) {
    return this.send({
      kind: 'reconcileLaunch',
      launchOperationId,
      resolution,
    });
  }

  // ---- panes ----------------------------------------------------------------------------------

  private save(
    layout: WorkspaceView['layout'],
    leaves: { id: string; title: string; associationId: string | null }[],
    focus: string | null,
    preferences?: Preferences,
  ) {
    const workspace = this.s.workspace;
    if (!workspace) return this.refuse('context_stale');
    return this.send({
      kind: 'saveWorkspace',
      workspaceId: workspace.id,
      expectedWorkspaceRevision: workspace.revision,
      label: workspace.label,
      layout,
      focus,
      pinned: workspace.pinned,
      leaves,
      preferences: preferences ?? workspace.preferences,
    });
  }

  private leaves() {
    return (this.s.workspace?.leaves ?? []).map((l) => ({
      id: l.id,
      title: l.title,
      associationId: l.session?.id ?? null,
    }));
  }

  /** A pane beside `target` (or the first pane of an empty workspace). */
  addPane(target: string | null, dir: Direction) {
    const workspace = this.s.workspace;
    if (!workspace) return this.refuse('context_stale');
    const added = this.newId();
    const leaves = [
      ...this.leaves(),
      { id: added, title: 'Pane', associationId: null },
    ];
    const layout =
      workspace.layout === null || target === null
        ? { kind: 'leaf' as const, id: added }
        : split(workspace.layout, target, added, dir);
    // A full or too-deep tree comes back unchanged: nothing was added.
    if (workspace.layout && layout === workspace.layout)
      return this.refuse('capacity_exceeded');
    return this.save(layout, leaves, added);
  }

  /** Removes an empty pane; a pane that shows a session is detached first. */
  closePane(leafId: string) {
    const workspace = this.s.workspace;
    if (!workspace?.layout) return this.refuse('context_stale');
    const leaf = workspace.leaves.find((l) => l.id === leafId);
    if (!leaf || leaf.session) return this.refuse('leaf_busy');
    const layout = remove(workspace.layout, leafId);
    return this.save(
      layout,
      this.leaves().filter((l) => l.id !== leafId),
      workspace.focus === leafId ? null : workspace.focus,
    );
  }

  resizeSplit(path: string, ratio: number) {
    const workspace = this.s.workspace;
    if (!workspace?.layout) return this.refuse('context_stale');
    return this.save(
      resize(workspace.layout, path, ratio),
      this.leaves(),
      workspace.focus,
    );
  }

  // ---- adoption of the feature, migration, rollback --------------------------------------------

  async setEnabled(on: boolean) {
    try {
      const overview = await this.client.setEnabled(on);
      this.set({ overview, notice: null });
      await this.refresh();
    } catch (error) {
      if (error instanceof ProjectsCallError) {
        // Declining the confirmation is not an error to show.
        if (error.failure.field !== 'declined')
          this.set({ notice: noticeOf(error.failure) });
      }
    }
  }

  async initializeEmpty() {
    try {
      await this.client.initializeEmpty();
      await this.refresh();
    } catch (error) {
      if (error instanceof ProjectsCallError)
        this.set({ notice: noticeOf(error.failure) });
    }
  }

  async previewMigration() {
    try {
      const preview = await this.client.previewMigration();
      this.set({ preview, notice: null });
    } catch (error) {
      if (error instanceof ProjectsCallError)
        this.set({ notice: noticeOf(error.failure), preview: null });
    }
  }

  async commitMigration(acceptAliases: boolean) {
    const preview = this.s.preview;
    if (!preview) return;
    try {
      await this.client.commitMigration(preview.previewId, acceptAliases);
      this.set({ preview: null, notice: null });
      await this.refresh();
    } catch (error) {
      if (error instanceof ProjectsCallError) {
        // The legacy files or the host moved: a new preview is needed, never a stale commit.
        this.set({ notice: noticeOf(error.failure), preview: null });
      }
    }
  }

  cancelPreview() {
    this.set({ preview: null });
  }

  async readRollback() {
    try {
      this.set({ rollback: await this.client.rollbackReport() });
    } catch (error) {
      if (error instanceof ProjectsCallError)
        this.set({ notice: noticeOf(error.failure) });
    }
  }

  async compactReceipts() {
    try {
      await this.client.compactReceipts();
      await this.refresh();
    } catch (error) {
      if (error instanceof ProjectsCallError)
        this.set({ notice: noticeOf(error.failure) });
    }
  }

  async openWindow() {
    try {
      await this.client.openWindow();
    } catch (error) {
      if (error instanceof ProjectsCallError)
        this.set({ notice: noticeOf(error.failure) });
    }
  }
}
