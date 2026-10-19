import {
  parseConsoleSessionId,
  type ConsoleSessionId,
} from '../../contracts/identities';
import type { StartReply } from '../../contracts/native';
import type { HostTerminal, TerminalInfo } from '../../contracts/terminal';
import * as desktop from '../../desktop/terminal-client';
import { TerminalController, type TerminalViewState } from './controller';
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
export interface WorkspaceState {
  sessions: readonly TerminalSession[];
  active?: ConsoleSessionId;
  creating: boolean;
  error?: string;
}
export type ControllerFactory = (
  host: HTMLElement,
  update: (state: TerminalViewState) => void,
  adopted?: TerminalInfo,
  title?: string,
) => TerminalController;
interface OwnedSession extends TerminalSession {
  controller?: TerminalController;
  closing?: Promise<void>;
  adopted?: TerminalInfo;
  /** Made by an earlier window and held by the host: drawn again once attached. */
  restored?: boolean;
}
export const SESSION_LIMIT = 16;

/** Volatile local presentation ownership, never managed-agent authority. */
export class TerminalSessionManager {
  private sessions: OwnedSession[] = [];
  private active?: ConsoleSessionId;
  private creating?: ConsoleSessionId;
  private error?: string;
  private ordinal = 0;
  private started = false;
  private disposed = false;
  private disposal?: Promise<void>;

  constructor(
    private readonly update: (state: WorkspaceState) => void,
    private readonly factory: ControllerFactory = (
      host,
      update,
      adopted,
      title,
    ) =>
      new TerminalController(
        host,
        createRenderer(host),
        update,
        adopted,
        title,
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
        creating: !!this.creating,
        error: this.error,
      });
  }
  start() {
    if (this.started || this.disposed) return;
    this.started = true;
    if (this.persistent) void this.restoreOrCreate();
    else this.create();
  }
  /** What the host holds is shown again; with nothing held, one new terminal. */
  private async restoreOrCreate() {
    const held = await desktop.listHostTerminals();
    if (this.disposed) return;
    if (held.length === 0) {
      this.create();
      return;
    }
    this.ordinal = held.filter((h) => h.native === null).length;
    held.forEach((entry, index) => this.adopt(entry, index === 0));
  }
  private adopt(entry: HostTerminal, activate: boolean) {
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
    if (activate) this.activate(id);
    else this.publish();
  }
  create(): ConsoleSessionId | undefined {
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
    this.activate(id);
    return id;
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
      );
      session.controller = controller;
      controller.setActive(this.active === id);
      void controller.start().finally(() => {
        this.created(id);
        const info = session.adopted;
        if (session.restored && info && session.view.state === 'running')
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
  activate(id: ConsoleSessionId) {
    if (this.disposed || !this.sessions.some((s) => s.id === id && !s.closing))
      return;
    const previous = this.active;
    this.active = id;
    if (previous !== id) {
      this.sessions
        .find((s) => s.id === previous)
        ?.controller?.setActive(false);
    }
    this.sessions.find((s) => s.id === id)?.controller?.setActive(true);
    this.publish();
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
      if (this.active === id) {
        this.active = undefined;
        const next =
          this.sessions.slice(index).find((s) => !s.closing) ??
          this.sessions
            .slice(0, index)
            .reverse()
            .find((s) => !s.closing);
        if (next) this.activate(next.id);
      }
      this.publish();
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
