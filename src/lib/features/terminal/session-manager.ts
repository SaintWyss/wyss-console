import {
  parseConsoleSessionId,
  type ConsoleSessionId,
} from '../../contracts/identities';
import type { StartReply } from '../../contracts/native';
import type { HostTerminal, TerminalInfo } from '../../contracts/terminal';
import * as desktop from '../../desktop/terminal-client';
import { TerminalController, type TerminalViewState } from './controller';
import {
  MAX_PANES,
  arrange,
  cycle,
  has,
  leaf,
  leaves,
  neighbour,
  readLayout,
  remove,
  resize,
  split,
  type Direction,
  type Node,
  type SavedLayout,
} from './layout';
import { createRenderer } from './renderer';

/** What marks a tab as an agent the Console started (F3): the registry's id and the words
 * to show. Nothing here is used to act on the process. */
export interface NativeTag {
  sessionId: string;
  agent: string;
  project: string;
  profile: string | null;
  throughShell: boolean;
}
export interface TerminalSession {
  id: ConsoleSessionId;
  title: string;
  view: TerminalViewState;
  native?: NativeTag;
}
/** One tab: a tree of panes, each a terminal. The focus is the pane the keyboard goes to. */
export interface TabView {
  id: string;
  root: Node<ConsoleSessionId>;
  focus: ConsoleSessionId;
}
export interface WorkspaceState {
  sessions: readonly TerminalSession[];
  /** The focused pane of the active tab. */
  active?: ConsoleSessionId;
  tabs: readonly TabView[];
  activeTab?: string;
  creating: boolean;
  error?: string;
}
export type ControllerFactory = (
  host: HTMLElement,
  update: (state: TerminalViewState) => void,
  adopted?: TerminalInfo,
  title?: string,
  cwd?: string,
  attach?: boolean,
) => TerminalController;
interface OwnedSession extends TerminalSession {
  controller?: TerminalController;
  closing?: Promise<void>;
  adopted?: TerminalInfo;
  /** Made by an earlier window and held by the host: drawn again once attached. */
  restored?: boolean;
  /** Where a shell that has to be made again starts (from a saved layout). */
  cwd?: string;
  /** Typed once the terminal runs: a launch's commands. */
  input?: string;
}
export const SESSION_LIMIT = 16;

/** Volatile local presentation ownership, never managed-agent authority. */
export class TerminalSessionManager {
  private sessions: OwnedSession[] = [];
  private tabs: TabView[] = [];
  private activeTab?: string;
  private saveTimer?: ReturnType<typeof setTimeout>;
  private active?: ConsoleSessionId;
  private creating?: ConsoleSessionId;
  private error?: string;
  private ordinal = 0;
  private started = false;
  private ready = false;
  private readyWaiters: Array<() => void> = [];
  private disposed = false;
  private disposal?: Promise<void>;

  constructor(
    private readonly update: (state: WorkspaceState) => void,
    private readonly factory: ControllerFactory = (
      host,
      update,
      adopted,
      title,
      cwd,
      attach,
    ) =>
      new TerminalController(
        host,
        createRenderer(host),
        update,
        adopted,
        title,
        cwd,
        attach,
      ),
    /**
     * Terminals belong to a host that outlives this window: they are listed from it when the
     * window opens, and leaving the window detaches them instead of closing them.
     */
    private readonly persistent = false,
  ) {}
  private publish() {
    if (!this.disposed)
      this.update({
        sessions: this.sessions.map(({ id, title, view, native }) => ({
          id,
          title,
          view,
          native,
        })),
        active: this.active,
        tabs: this.tabs.map((t) => ({ ...t })),
        activeTab: this.activeTab,
        creating: !!this.creating,
        error: this.error,
      });
  }
  private tabOfSession(id: ConsoleSessionId): TabView | undefined {
    return this.tabs.find((t) => has(t.root, id));
  }
  /** A new tab with one pane. */
  private addTab(id: ConsoleSessionId) {
    this.tabs.push({
      id: crypto.randomUUID(),
      root: leaf(id),
      focus: id,
    });
  }
  /** Puts a new session in the layout: its own tab, or beside `beside` in that pane's tab. */
  private place(
    id: ConsoleSessionId,
    beside?: { of: ConsoleSessionId; dir: Direction },
  ) {
    const tab = beside && this.tabOfSession(beside.of);
    if (tab && beside) {
      const root = split(tab.root, beside.of, id, beside.dir);
      if (root !== tab.root) {
        tab.root = root;
        return;
      }
    }
    this.addTab(id);
  }
  /** Writes the layout down, a moment after the last change (T2). */
  private changed() {
    if (!this.persistent || this.disposed) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      void desktop.writeLayout(this.snapshot());
    }, 400);
  }
  private snapshot(): SavedLayout | null {
    const key = (id: ConsoleSessionId) =>
      this.sessions.find((s) => s.id === id)?.view.info?.terminal_id;
    const tabs = this.tabs
      .filter((t) => leaves(t.root).every((l) => key(l)))
      .map((t) => ({
        root: mapIds(t.root, (id) => key(id) as string),
        focus: key(t.focus) as string,
      }));
    if (tabs.length === 0) return null;
    const terminals: SavedLayout['terminals'] = {};
    for (const t of this.tabs)
      for (const id of leaves(t.root)) {
        const s = this.sessions.find((x) => x.id === id);
        const k = key(id);
        if (s && k)
          terminals[k] = {
            title: s.title,
            cwd: s.view.info?.cwd ?? '',
            native: !!s.native,
          };
      }
    const at = this.tabs.findIndex((t) => t.id === this.activeTab);
    return {
      version: 1,
      tabs,
      active: Math.max(0, at),
      terminals,
    };
  }
  start() {
    if (this.started || this.disposed) return;
    this.started = true;
    if (this.persistent)
      void this.restoreOrCreate().finally(() => this.markReady());
    else {
      this.create();
      this.markReady();
    }
  }
  /**
   * Runs `run` once the terminals this manager starts with are in place: restored from the host
   * and laid out, or the first one created. Runs at once when that has already happened.
   */
  whenReady(run: () => void) {
    if (this.ready) run();
    else this.readyWaiters.push(run);
  }
  private markReady() {
    if (this.ready) return;
    this.ready = true;
    const waiters = this.readyWaiters;
    this.readyWaiters = [];
    for (const run of waiters) run();
  }
  /**
   * What the host holds is shown again, laid out as the saved layout says (T2); a terminal the
   * layout names that no longer exists (the host restarted) is made again as a new shell with
   * its title and folder; with nothing at all, one new terminal.
   */
  private async restoreOrCreate() {
    const held = await desktop.listHostTerminals();
    const saved = readLayout(await desktop.readLayout());
    if (this.disposed) return;
    this.ordinal = held.filter((h) => h.native === null).length;
    const byTerminal = new Map(
      held.map((h) => [h.terminal.terminal_id as string, h]),
    );
    const placed = new Set<string>();
    for (const tab of saved?.tabs ?? []) {
      const sessionOf = new Map<string, ConsoleSessionId>();
      for (const terminal of leaves(tab.root)) {
        const entry = byTerminal.get(terminal);
        if (entry) {
          placed.add(terminal);
          sessionOf.set(terminal, this.adopt(entry));
        } else {
          const was = saved?.terminals[terminal];
          // An agent is never started again behind the user's back.
          if (was && !was.native)
            sessionOf.set(terminal, this.remake(was.title, was.cwd));
        }
      }
      let root: Node<ConsoleSessionId> | null = mapIds(
        tab.root,
        (t) => sessionOf.get(t) ?? ('' as ConsoleSessionId),
      );
      for (const [, id] of [...sessionOf].filter(([, v]) => !v))
        root = root && remove(root, id);
      root = prune(root, sessionOf);
      if (!root) continue;
      const focus = sessionOf.get(tab.focus) ?? leaves(root)[0]!;
      this.tabs.push({ id: crypto.randomUUID(), root, focus });
    }
    // What the host holds and the layout did not name: a tab each.
    for (const entry of held)
      if (!placed.has(entry.terminal.terminal_id))
        this.addTab(this.adopt(entry));
    if (this.sessions.length === 0) {
      this.create();
      return;
    }
    const wanted = saved
      ? this.tabs[Math.min(saved.active, this.tabs.length - 1)]
      : undefined;
    this.activateTabView(wanted ?? this.tabs[0]!);
  }
  private adopt(entry: HostTerminal): ConsoleSessionId {
    const id = parseConsoleSessionId(crypto.randomUUID());
    const tag = entry.native;
    this.sessions.push({
      id,
      title: entry.title,
      view: { state: 'starting' },
      adopted: entry.terminal,
      restored: true,
      native: tag
        ? {
            sessionId: tag.session_id,
            agent: tag.agent_name,
            project: tag.project,
            profile: tag.profile,
            throughShell: tag.through_shell,
          }
        : undefined,
    });
    return id;
  }
  /** A shell the layout remembers and the host no longer has. */
  private remake(title: string, cwd: string): ConsoleSessionId {
    const id = parseConsoleSessionId(crypto.randomUUID());
    this.sessions.push({
      id,
      title,
      view: { state: 'starting' },
      cwd: cwd || undefined,
    });
    return id;
  }
  private activateTabView(tab: TabView) {
    this.publish();
    this.activate(tab.focus);
  }
  create(beside?: {
    of: ConsoleSessionId;
    dir: Direction;
  }): ConsoleSessionId | undefined {
    if (this.disposed || this.creating) return;
    if (this.sessions.length >= SESSION_LIMIT) {
      this.error =
        'Terminal limit reached (16). Close a terminal to create another.';
      this.publish();
      return;
    }
    const id = parseConsoleSessionId(crypto.randomUUID());
    this.creating = id;
    this.error = undefined;
    this.sessions.push({
      id,
      title: `Terminal ${++this.ordinal}`,
      view: { state: 'starting' },
    });
    this.place(id, beside);
    this.activate(id);
    return id;
  }
  /**
   * A launch's terminal (ADR 0059): a new tab titled as the launch, in its folder, with its
   * commands typed once its shell runs.
   */
  launch(
    title: string,
    cwd: string,
    input: string,
  ): ConsoleSessionId | undefined {
    const id = this.create();
    const session = id && this.sessions.find((s) => s.id === id);
    if (!session) return;
    session.title = title;
    session.cwd = cwd;
    session.input = input;
    this.publish();
    return id;
  }
  /** A new terminal beside the pane `of`, in its tab. */
  split(of: ConsoleSessionId, dir: Direction): ConsoleSessionId | undefined {
    const tab = this.tabOfSession(of);
    if (!tab) return;
    if (leaves(tab.root).length >= MAX_PANES) {
      this.error = `A tab holds at most ${MAX_PANES} panes.`;
      this.publish();
      return;
    }
    return this.create({ of, dir });
  }
  /**
   * A tab for an agent the desktop started: the terminal exists already, and this one only
   * reads it and takes its input. It counts against the same limit as any terminal.
   */
  createNative(reply: StartReply): ConsoleSessionId | undefined {
    if (this.disposed) return;
    if (this.sessions.length >= SESSION_LIMIT) {
      this.error =
        'Terminal limit reached (16). Close a terminal to open another.';
      this.publish();
      return;
    }
    const id = parseConsoleSessionId(crypto.randomUUID());
    const session = reply.session;
    const folder = session.project.split('/').filter(Boolean).pop() ?? '/';
    this.error = undefined;
    this.sessions.push({
      id,
      title: `${session.agent_name} · ${folder}`,
      view: { state: 'starting' },
      adopted: reply.terminal,
      native: {
        sessionId: session.id,
        agent: session.agent_name,
        project: session.project,
        profile: session.profile?.name ?? null,
        throughShell: session.through_shell,
      },
    });
    this.addTab(id);
    this.activate(id);
    return id;
  }
  /** The tab of a native session, when this window has one. */
  tabOf(sessionId: string): ConsoleSessionId | undefined {
    return this.sessions.find((s) => s.native?.sessionId === sessionId)?.id;
  }
  /** Called once when this session's persistent panel obtains its DOM host. */
  attach(id: ConsoleSessionId, host: HTMLElement) {
    const session = this.sessions.find((s) => s.id === id);
    if (!session || session.controller || session.closing || this.disposed)
      return;
    try {
      const controller = this.factory(
        host,
        (view) => {
          session.view = view;
          this.publish();
        },
        session.adopted,
        session.title,
        session.cwd,
        session.restored,
      );
      session.controller = controller;
      controller.setActive(this.shown(id), this.active === id);
      void controller.start().finally(() => {
        this.created(id);
        if (session.input && session.view.state === 'running')
          controller.type(session.input);
        session.input = undefined;
        const info = session.adopted;
        // A screen the host's parser kept was drawn already; only when none was, the program is
        // asked to draw its own again.
        if (
          session.restored &&
          info &&
          session.view.state === 'running' &&
          session.view.attach?.mode !== 'screen'
        )
          void desktop.redrawTerminal(info.terminal_id);
      });
    } catch {
      session.view = { state: 'failed', error: 'renderer_error' };
      this.created(id);
    }
  }
  private created(id: ConsoleSessionId) {
    if (this.creating === id) this.creating = undefined;
    this.publish();
  }
  /** Whether a session is in the active tab (all its panes are drawn). */
  private shown(id: ConsoleSessionId): boolean {
    const tab = this.tabs.find((t) => t.id === this.activeTab);
    return !!tab && has(tab.root, id);
  }
  activate(id: ConsoleSessionId) {
    if (this.disposed || !this.sessions.some((s) => s.id === id && !s.closing))
      return;
    const tab = this.tabOfSession(id);
    if (!tab) return;
    tab.focus = id;
    this.activeTab = tab.id;
    this.active = id;
    for (const s of this.sessions)
      s.controller?.setActive(this.shown(s.id), s.id === id);
    this.publish();
    this.changed();
  }
  activateTab(tabId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (tab) this.activate(tab.focus);
  }
  /** A step through the tabs, wrapping. */
  stepTab(step: 1 | -1) {
    if (this.tabs.length < 2) return;
    const at = this.tabs.findIndex((t) => t.id === this.activeTab);
    this.activateTab(
      this.tabs[(at + step + this.tabs.length) % this.tabs.length]!.id,
    );
  }
  /** The `n`-th tab (from 1), or the last when `n` is past the end. */
  jumpTab(n: number) {
    const tab = this.tabs[Math.min(n, this.tabs.length) - 1];
    if (tab) this.activateTab(tab.id);
  }
  /** Moves the focus to the pane that is that way, or steps through them. */
  focusPane(way: 'left' | 'right' | 'up' | 'down' | 'next' | 'previous') {
    const tab = this.tabs.find((t) => t.id === this.activeTab);
    if (!tab) return;
    const to =
      way === 'next'
        ? cycle(tab.root, tab.focus, 1)
        : way === 'previous'
          ? cycle(tab.root, tab.focus, -1)
          : neighbour(tab.root, tab.focus, way);
    if (to) this.activate(to);
  }
  /** A divider was dragged: its split gets this share. */
  resizeSplit(tabId: string, path: string, ratio: number) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) return;
    tab.root = resize(tab.root, path, ratio);
    this.publish();
    this.changed();
  }
  /** Closes every pane of a tab. */
  closeTab(tabId: string): Promise<void> {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) return Promise.resolve();
    return Promise.all(leaves(tab.root).map((id) => this.close(id))).then(
      () => undefined,
    );
  }
  /** Search in one pane's text. */
  search(
    id: ConsoleSessionId,
    query: string,
    options: { caseSensitive: boolean; step: 1 | -1 | 0 },
  ) {
    return (
      this.sessions
        .find((s) => s.id === id)
        ?.controller?.search(query, options) ?? {
        total: 0,
        current: 0,
      }
    );
  }
  clearSearch(id: ConsoleSessionId) {
    this.sessions.find((s) => s.id === id)?.controller?.clearSearch();
    this.sessions.find((s) => s.id === id)?.controller?.setActive(true, true);
  }
  /** The layout as it is drawn: where each pane of the active tab sits. */
  arrangement(tabId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    return tab ? arrange(tab.root) : undefined;
  }
  close(id: ConsoleSessionId): Promise<void> {
    const session = this.sessions.find((s) => s.id === id);
    if (!session) return Promise.resolve();
    if (session.closing) return session.closing;
    // Stop input synchronously; the controller's memoized close owns late-create cleanup.
    session.view = { ...session.view, state: 'closing' };
    const cleanup = session.controller?.dispose() ?? Promise.resolve();
    session.closing = cleanup.then(() => {
      const index = this.sessions.indexOf(session);
      if (index < 0) return;
      this.sessions.splice(index, 1);
      if (this.creating === id) this.creating = undefined;
      const tab = this.tabOfSession(id);
      let next: ConsoleSessionId | undefined;
      if (tab) {
        const rest = remove(tab.root, id);
        if (rest) {
          // The neighbour takes the focus: the pane that was beside it.
          next =
            tab.focus === id
              ? (neighbour(tab.root, id, 'left') ??
                neighbour(tab.root, id, 'up') ??
                neighbour(tab.root, id, 'right') ??
                neighbour(tab.root, id, 'down') ??
                leaves(rest)[0])
              : tab.focus;
          tab.root = rest;
          tab.focus = next as ConsoleSessionId;
        } else {
          const at = this.tabs.indexOf(tab);
          this.tabs.splice(at, 1);
          if (this.activeTab === tab.id) {
            const to = this.tabs[at] ?? this.tabs[at - 1];
            next = to?.focus;
            this.activeTab = to?.id;
          }
        }
      }
      if (this.active === id) {
        this.active = undefined;
        if (next) this.activate(next);
      }
      this.publish();
      this.changed();
    });
    this.publish();
    return session.closing;
  }
  dispose(): Promise<void> {
    if (this.disposal) return this.disposal;
    this.disposed = true;
    // A host keeps the terminals past the window: they are let go of, not closed.
    this.disposal = Promise.all(
      this.sessions.map((s) =>
        this.persistent
          ? (s.controller?.detach() ?? Promise.resolve())
          : this.close(s.id),
      ),
    ).then(() => undefined);
    return this.disposal;
  }
}

/** The tree with every leaf renamed. */
function mapIds<A extends string, B extends string>(
  node: Node<A>,
  to: (id: A) => B,
): Node<B> {
  return node.kind === 'leaf'
    ? leaf(to(node.id))
    : { ...node, a: mapIds(node.a, to), b: mapIds(node.b, to) };
}

/** The tree without the leaves that stand for a terminal that could not be placed. */
function prune(
  node: Node<ConsoleSessionId> | null,
  placed: Map<string, ConsoleSessionId>,
): Node<ConsoleSessionId> | null {
  if (!node) return null;
  if (node.kind === 'leaf')
    return node.id && [...placed.values()].includes(node.id) ? node : null;
  const a = prune(node.a, placed);
  const b = prune(node.b, placed);
  return a && b ? { ...node, a, b } : (a ?? b);
}
