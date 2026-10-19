import {
  parseConsoleSessionId,
  type ConsoleSessionId,
} from '../../contracts/identities';
import type { StartReply } from '../../contracts/native';
import type { TerminalInfo } from '../../contracts/terminal';
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
) => TerminalController;
interface OwnedSession extends TerminalSession {
  controller?: TerminalController;
  closing?: Promise<void>;
  adopted?: TerminalInfo;
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
    private readonly factory: ControllerFactory = (host, update, adopted) =>
      new TerminalController(host, createRenderer(host), update, adopted),
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
    this.create();
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
      );
      session.controller = controller;
      controller.setActive(this.active === id);
      void controller.start().finally(() => this.created(id));
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
    this.disposal = Promise.all(
      this.sessions.map((s) => this.close(s.id)),
    ).then(() => undefined);
    return this.disposal;
  }
}
