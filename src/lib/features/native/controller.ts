import type {
  AgentId,
  Launcher,
  NativeFailure,
  NativeSession,
  Preview,
  Profile,
  ProfileBody,
  StartReply,
} from '../../contracts/native';
import {
  NativeCallError,
  chooseProject,
  createProfile,
  deleteProfile,
  duplicateProfile,
  exportProfile,
  forgetNative,
  importProfile,
  previewProfile,
  readLauncher,
  readNativeSessions,
  readProfiles,
  restartNative,
  restoreProfile,
  saveProfile,
  setProjectDefault,
  startNative,
  stopNative,
} from '../../desktop/native-client';
import { currentWorkspace } from './bridge';
import { restorable } from './present';

export interface NativeClient {
  launcher(): Promise<Launcher>;
  sessions(sample: boolean): Promise<NativeSession[]>;
  profiles(): Promise<Profile[]>;
  preview(agent: AgentId, profile: string | null): Promise<Preview>;
  start(
    agent: AgentId,
    project: string,
    profile: string | null,
  ): Promise<StartReply>;
  restart(id: string): Promise<StartReply>;
  stop(id: string): Promise<void>;
  forget(id: string): Promise<void>;
  chooseProject(): Promise<string | null>;
  setDefault(project: string, profile: string | null): Promise<void>;
  createProfile(body: ProfileBody): Promise<string>;
  saveProfile(id: string, body: ProfileBody): Promise<void>;
  deleteProfile(id: string): Promise<void>;
  duplicateProfile(id: string): Promise<string>;
  restoreProfile(id: string, version: number): Promise<void>;
  exportProfile(id: string): Promise<boolean>;
  importProfile(): Promise<string | null>;
}

/** What a terminal that has not been fitted yet is made with; it is resized at once. */
const FIRST_SIZE = { rows: 24, cols: 80 };

const desktopClient: NativeClient = {
  launcher: readLauncher,
  sessions: readNativeSessions,
  profiles: readProfiles,
  preview: previewProfile,
  start: (agent, project, profile) =>
    startNative(agent, project, profile, FIRST_SIZE),
  restart: (id) => restartNative(id, FIRST_SIZE),
  stop: stopNative,
  forget: forgetNative,
  chooseProject,
  setDefault: setProjectDefault,
  createProfile,
  saveProfile,
  deleteProfile,
  duplicateProfile,
  restoreProfile,
  exportProfile,
  importProfile,
};

/** The sessions as they are drawn: usage rounded to a whole percent and a megabyte. */
function shownKey(sessions: NativeSession[]): string {
  return JSON.stringify(
    sessions.map((s) => ({
      ...s,
      usage: s.usage && {
        cpu:
          s.usage.cpu_percent === null ? null : Math.round(s.usage.cpu_percent),
        mb: Math.round(s.usage.rss_bytes / 1048576),
        n: s.usage.processes,
      },
    })),
  );
}

/** How often the registry is read while the overview is shown, and while something runs. */
export const OVERVIEW_MS = 2000;
export const BACKGROUND_MS = 4000;

export interface NativeState {
  phase: 'idle' | 'loading' | 'ready' | 'failed';
  launcher: Launcher | null;
  sessions: NativeSession[];
  profiles: Profile[];
  starting: boolean;
  failure: NativeFailure | null;
  /** Something that happened and is worth one line, such as an import. */
  notice: string | null;
  /** The profile being edited: an id, `new`, or none. */
  editing: string | null;
}

/**
 * The one owner of native sessions in the window. It asks the desktop; the desktop starts,
 * stops and records. It reads the registry while the overview is shown (with resources) and,
 * more slowly, while something runs (without them); it never reads while nothing does and
 * the view is hidden.
 */
export class NativeController {
  private phase: NativeState['phase'] = 'idle';
  private launcher: Launcher | null = null;
  private sessions: NativeSession[] = [];
  private profiles: Profile[] = [];
  private starting = false;
  private failure: NativeFailure | null = null;
  private notice: string | null = null;
  private editing: string | null = null;
  private visible = false;
  private disposed = false;
  private reading = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private lastShown = '';

  constructor(
    private readonly publish: (state: NativeState) => void,
    /** Called after an agent was started: the window shows its terminal. */
    private readonly onstarted: (reply: StartReply) => void = () => undefined,
    private readonly client: NativeClient = desktopClient,
    private readonly every = {
      overview: OVERVIEW_MS,
      background: BACKGROUND_MS,
    },
  ) {}

  state(): NativeState {
    return {
      phase: this.phase,
      launcher: this.launcher,
      sessions: this.sessions,
      profiles: this.profiles,
      starting: this.starting,
      failure: this.failure,
      notice: this.notice,
      editing: this.editing,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }
  private fail(error: unknown) {
    this.failure =
      error instanceof NativeCallError
        ? error.failure
        : { code: 'internal', reasons: [] };
    this.emit();
  }

  /** Reads what the launcher offers and the profiles, once at start. */
  start0() {
    if (this.disposed || this.phase !== 'idle') return;
    this.phase = 'loading';
    this.emit();
    void Promise.all([
      this.client.launcher(),
      this.client.profiles(),
      this.client.sessions(false),
    ]).then(
      ([launcher, profiles, sessions]) => {
        if (this.disposed) return;
        this.launcher = launcher;
        this.profiles = profiles;
        this.sessions = sessions;
        this.phase = 'ready';
        this.emit();
        this.schedule();
      },
      (error: unknown) => {
        this.phase = 'failed';
        this.fail(error);
      },
    );
  }

  /** Whether the overview is shown. Resources are sampled only then. */
  setVisible(visible: boolean) {
    if (this.disposed || visible === this.visible) return;
    this.visible = visible;
    if (visible) {
      this.start0();
      if (this.phase === 'ready') {
        void this.refreshLauncher();
        this.refreshSessions();
      }
    } else this.schedule();
  }

  private anyRunning(): boolean {
    return this.sessions.some(
      (s) => s.state === 'running' || s.state === 'requested',
    );
  }
  private stopTimer() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
  private schedule() {
    this.stopTimer();
    if (this.disposed || this.phase !== 'ready') return;
    if (!this.visible && !this.anyRunning()) return;
    this.timer = setTimeout(
      () => this.refreshSessions(),
      this.visible ? this.every.overview : this.every.background,
    );
  }

  refreshSessions() {
    if (this.disposed || this.reading) return;
    this.stopTimer();
    this.reading = true;
    void this.client.sessions(this.visible).then(
      (sessions) => {
        this.reading = false;
        if (this.disposed) return;
        // What is drawn is rounded (a percent, a megabyte): a read that would draw the same
        // page changes nothing, so the window does no work for it.
        const shown = shownKey(sessions);
        if (shown !== this.lastShown) {
          this.lastShown = shown;
          this.sessions = sessions;
          this.emit();
        }
        this.schedule();
      },
      () => {
        this.reading = false;
        this.schedule();
      },
    );
  }
  async refreshLauncher() {
    try {
      this.launcher = await this.client.launcher();
      this.emit();
    } catch (error) {
      this.fail(error);
    }
  }
  async refreshProfiles() {
    try {
      this.profiles = await this.client.profiles();
      this.emit();
    } catch (error) {
      this.fail(error);
    }
  }

  clearFailure() {
    this.failure = null;
    this.notice = null;
    this.emit();
  }

  // ---- sessions ---------------------------------------------------------------------------

  async choose(): Promise<string | null> {
    try {
      const path = await this.client.chooseProject();
      await this.refreshLauncher();
      return path;
    } catch (error) {
      this.fail(error);
      return null;
    }
  }
  preview(agent: AgentId, profile: string | null): Promise<Preview> {
    return this.client.preview(agent, profile);
  }

  /** Starts the agent; the window gets its terminal. One start at a time. */
  async startAgent(
    agent: AgentId,
    project: string,
    profile: string | null,
  ): Promise<boolean> {
    if (this.starting || this.disposed) return false;
    this.starting = true;
    this.failure = null;
    this.emit();
    try {
      const reply = await this.client.start(agent, project, profile);
      this.afterStart(reply);
      return true;
    } catch (error) {
      this.fail(error);
      return false;
    } finally {
      this.starting = false;
      this.emit();
    }
  }
  private afterStart(reply: StartReply) {
    this.sessions = [
      ...this.sessions.filter((s) => s.id !== reply.session.id),
      reply.session,
    ];
    this.lastShown = shownKey(this.sessions);
    this.onstarted(reply);
    this.refreshSessions();
  }
  async restart(id: string): Promise<boolean> {
    if (this.starting || this.disposed) return false;
    this.starting = true;
    this.failure = null;
    this.emit();
    try {
      this.afterStart(await this.client.restart(id));
      return true;
    } catch (error) {
      this.fail(error);
      return false;
    } finally {
      this.starting = false;
      this.emit();
    }
  }
  /**
   * Starts again, one after another, what was lost when the window closed: same agent,
   * project and profile, each a new session. Nothing is claimed to have survived.
   */
  async restoreLost(): Promise<number> {
    let started = 0;
    for (const lost of restorable(this.sessions)) {
      if (!(await this.restart(lost.id))) break;
      started += 1;
    }
    return started;
  }
  /** Removes the records of every session that is not running. */
  async forgetFinished() {
    for (const session of this.sessions.filter(
      (s) => s.state !== 'running' && s.state !== 'requested',
    ))
      try {
        await this.client.forget(session.id);
      } catch (error) {
        this.fail(error);
        break;
      }
    this.refreshSessions();
  }
  async stop(id: string) {
    try {
      await this.client.stop(id);
    } catch (error) {
      this.fail(error);
    }
    this.refreshSessions();
  }
  async forget(id: string) {
    try {
      await this.client.forget(id);
    } catch (error) {
      this.fail(error);
    }
    this.refreshSessions();
  }
  /** Whether this window has a tab for the session. */
  hasTab(id: string): boolean {
    return !!currentWorkspace()?.tabOf(id);
  }
  /** Shows the terminal of a session this window has a tab for. */
  showTerminal(id: string): boolean {
    const workspace = currentWorkspace();
    const tab = workspace?.tabOf(id);
    if (!workspace || !tab) return false;
    workspace.activate(tab);
    return true;
  }
  async setProjectDefault(project: string, profile: string | null) {
    try {
      await this.client.setDefault(project, profile);
      await this.refreshLauncher();
    } catch (error) {
      this.fail(error);
    }
  }

  // ---- profiles ---------------------------------------------------------------------------

  edit(id: string | null) {
    this.editing = id;
    this.failure = null;
    this.emit();
  }
  /** Saves a new profile (`id` null) or a new version of one. True when it was saved. */
  async saveProfile(id: string | null, body: ProfileBody): Promise<boolean> {
    try {
      if (id === null) this.editing = await this.client.createProfile(body);
      else await this.client.saveProfile(id, body);
      this.failure = null;
      await this.refreshProfiles();
      return true;
    } catch (error) {
      this.fail(error);
      return false;
    }
  }
  async removeProfile(id: string) {
    try {
      await this.client.deleteProfile(id);
      if (this.editing === id) this.editing = null;
      await this.refreshProfiles();
    } catch (error) {
      this.fail(error);
    }
  }
  async duplicate(id: string) {
    try {
      this.editing = await this.client.duplicateProfile(id);
      await this.refreshProfiles();
    } catch (error) {
      this.fail(error);
    }
  }
  async restore(id: string, version: number) {
    try {
      await this.client.restoreProfile(id, version);
      await this.refreshProfiles();
    } catch (error) {
      this.fail(error);
    }
  }
  async exportOne(id: string) {
    try {
      this.notice = (await this.client.exportProfile(id))
        ? 'Profile exported.'
        : null;
      this.emit();
    } catch (error) {
      this.fail(error);
    }
  }
  async importOne() {
    try {
      const id = await this.client.importProfile();
      if (id) {
        this.editing = id;
        this.notice =
          'Profile imported. It grants nothing: read what it asks for before using it.';
        await this.refreshProfiles();
      }
    } catch (error) {
      this.fail(error);
    }
  }

  dispose() {
    this.disposed = true;
    this.stopTimer();
  }
}
