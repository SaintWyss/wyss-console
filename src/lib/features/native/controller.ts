import type {
  AgentId,
  Launcher,
  Readiness,
  NativeFailure,
  NativeSession,
  Preview,
  Profile,
  ProfileBody,
  StartReply,
} from '../../contracts/native';
import { msg } from '../../i18n/msg';
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
  prepareHandoff,
  readLauncher,
  readReadiness,
  saveHandoff,
  type ArsenalStart,
  type HandoffBrief,
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
import {
  START_UNRECORDED,
  STOP_UNRECORDED,
  isM9Agent,
  restorable,
} from './present';

export interface NativeClient {
  launcher(): Promise<Launcher>;
  readiness(): Promise<Readiness>;
  sessions(sample: boolean): Promise<NativeSession[]>;
  profiles(): Promise<Profile[]>;
  preview(
    agent: AgentId,
    profile: string | null,
    arsenal?: ArsenalStart | null,
  ): Promise<Preview>;
  start(
    agent: AgentId,
    project: string,
    profile: string | null,
    brief?: string | null,
    preview?: Pick<Preview, 'plan_token' | 'provider_version'> | null,
    arsenal?: ArsenalStart | null,
    delegation?: boolean,
  ): Promise<StartReply>;
  prepareHandoff(id: string): Promise<HandoffBrief>;
  saveHandoff(text: string): Promise<boolean>;
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
  readiness: readReadiness,
  sessions: readNativeSessions,
  profiles: readProfiles,
  preview: previewProfile,
  start: (
    agent,
    project,
    profile,
    brief = null,
    preview = null,
    arsenal = null,
    delegation = false,
  ) =>
    startNative(
      agent,
      project,
      profile,
      FIRST_SIZE,
      brief,
      preview,
      arsenal,
      delegation,
    ),
  prepareHandoff,
  saveHandoff,
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

/** A session whose process is still there, or on its way to being there. */
const isLive = (s: NativeSession) =>
  s.state === 'running' || s.state === 'requested';

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
  /** The M9 store is not confirmed since a failed read or write, until a read answers again. */
  m9Uncertain: boolean;
  /** Whether OpenCode and Hermes may not start now: the store is degraded or not confirmed. */
  m9Blocked: boolean;
  /** A current launcher confirmation is already in flight. */
  checkingLauncher: boolean;
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
  private readiness: Readiness | null = null;
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
  private m9Uncertain = true;
  /** The number of the newest launcher read: an older answer never overrides it. */
  private launcherSeq = 0;
  private catalogueSeq = 0;
  private launcherPending: { seq: number; slow: boolean } | null = null;
  private sessionsSeq = 0;
  private mutations = 0;
  private rereadSessions = false;

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
      m9Uncertain: this.m9Uncertain,
      m9Blocked: this.blocksM9(),
      checkingLauncher:
        this.launcherPending?.seq === this.launcherSeq &&
        !this.launcherPending.slow,
    };
  }
  private emit() {
    if (!this.disposed) this.publish(this.state());
  }
  private fail(error: unknown) {
    if (this.disposed) return;
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
    void this.load().catch((error: unknown) => {
      if (this.disposed) return;
      this.withdrawReadiness();
      this.phase = 'failed';
      this.readFailed(error);
    });
  }
  /**
   * The first load discovers the catalogue once, then reads profiles, sessions and readiness.
   * A listing that observes an ending may have written the store, so readiness is confirmed last.
   */
  private async load() {
    const sessionsSeq = this.sessionsSeq;
    // Discovery attaches the host on first load. The lightweight read deliberately cannot
    // initialize one, so it starts only after that attachment has completed.
    await this.refreshCatalogue();
    if (this.disposed) return;
    const first = this.readReadiness();
    const [, profiles, sessions] = await Promise.all([
      first,
      this.client.profiles(),
      this.client.sessions(false),
    ]);
    if (this.disposed) return;
    this.profiles = profiles;
    if (sessionsSeq === this.sessionsSeq) {
      this.sessions = sessions;
      this.lastShown = shownKey(sessions);
    }
    // Only a listing that shows an ending can have been the one that wrote the store.
    if (sessionsSeq === this.sessionsSeq && sessions.some((s) => !isLive(s)))
      await this.refreshLauncher();
    if (this.disposed) return;
    this.phase = this.launcher ? 'ready' : 'failed';
    this.emit();
    this.schedule();
  }

  /** Whether the overview is shown. Resources are sampled only then. */
  setVisible(visible: boolean) {
    if (this.disposed || visible === this.visible) return;
    this.stopTimer();
    this.visible = visible;
    if (visible) {
      this.start0();
      if (this.phase === 'ready') {
        this.withdrawReadiness();
        void this.refreshLauncher();
        this.refreshSessions();
      }
    }
    this.schedule();
  }

  /** Whether an M9 start must wait: the store is degraded, or not confirmed since a failure. */
  private blocksM9(): boolean {
    return this.m9Uncertain || this.readiness?.m9_native_status !== 'ready';
  }
  /**
   * An M9 operation that failed on its record may have had its effect already: the store is
   * unconfirmed until a read that comes after this answers, and the message says what may have
   * happened. Any other failure is shown as it is.
   */
  private storeFailed(error: unknown, reason: string) {
    if (this.disposed) return;
    if (
      error instanceof NativeCallError &&
      error.failure.code === 'store_unavailable'
    ) {
      this.m9Uncertain = true;
      this.failure = {
        code: 'store_unavailable',
        reasons: [...error.failure.reasons, reason],
      };
      this.emit();
    } else this.fail(error);
  }
  /** Refuses an M9 start (or restart) here, without asking the desktop, while it must wait. */
  private refuseM9(agent: AgentId | undefined): boolean {
    if (!agent || !isM9Agent(agent) || !this.blocksM9()) return false;
    this.failure = { code: 'store_unavailable', reasons: [] };
    this.emit();
    return true;
  }
  private anyRunning(): boolean {
    return this.sessions.some(
      (s) => s.state === 'running' || s.state === 'requested',
    );
  }
  /** No read started before a mutation may confirm its outcome or restore old sessions. */
  private withdrawReadiness() {
    this.launcherSeq++;
    this.sessionsSeq++;
    this.m9Uncertain = true;
    this.emit();
  }
  private beginMutation() {
    this.mutations++;
    this.withdrawReadiness();
  }
  private endMutation() {
    this.mutations--;
    if (this.disposed) return;
    this.withdrawReadiness();
    if (this.mutations === 0) {
      void this.refreshLauncher();
      this.refreshSessions();
      this.schedule();
    }
  }
  private stopTimer() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
  private schedule() {
    if (
      this.disposed ||
      this.phase !== 'ready' ||
      (!this.visible && !this.anyRunning())
    ) {
      this.stopTimer();
      return;
    }
    if (this.timer !== null) return;
    // One heartbeat, independent of either IPC completing. A hung sessions read cannot
    // prevent readiness checks, and neither slow read can accumulate background requests.
    this.timer = setTimeout(
      () => {
        this.timer = null;
        const launcherSeq = this.launcherSeq;
        this.readSessions(false);
        // Let an immediately available ending request its own confirmation first. Slow
        // sessions IPC does not hold this heartbeat up; it still checks readiness now.
        void Promise.resolve().then(() => {
          if (
            !this.disposed &&
            this.visible &&
            launcherSeq === this.launcherSeq
          )
            this.pollLauncher();
        });
        this.schedule();
      },
      this.visible ? this.every.overview : this.every.background,
    );
  }
  private pollLauncher() {
    if (this.launcherPending?.seq === this.launcherSeq) {
      // A confirmation still pending at the next heartbeat is no longer a trusted ready.
      // Keep waiting for that request rather than starting another one every interval.
      if (!this.launcherPending.slow) {
        this.launcherPending.slow = true;
        this.m9Uncertain = true;
        this.emit();
      }
    } else if (this.mutations === 0) void this.refreshLauncher();
  }

  refreshSessions() {
    this.readSessions(true);
  }
  private readSessions(fresh: boolean) {
    if (this.disposed) return;
    if (this.mutations > 0) {
      this.rereadSessions = true;
      return;
    }
    // A read already running may have started before the change this call reports: the
    // next read starts once it has answered, so the last answer is never older than the change.
    if (this.reading) {
      if (fresh) {
        this.sessionsSeq++;
        this.rereadSessions = true;
      }
      return;
    }
    this.rereadSessions = false;
    this.reading = true;
    const seq = ++this.sessionsSeq;
    void this.client.sessions(this.visible).then(
      (sessions) => {
        this.reading = false;
        if (this.disposed) return;
        // What is drawn is rounded (a percent, a megabyte): a read that would draw the same
        // page changes nothing, so the window does no work for it.
        const shown = shownKey(sessions);
        if (seq === this.sessionsSeq && shown !== this.lastShown) {
          this.lastShown = shown;
          this.applySessions(sessions);
          this.emit();
        }
        this.settleRead();
      },
      () => {
        this.reading = false;
        this.settleRead();
      },
    );
  }
  /** After a read: a request made while it ran is served now; otherwise the next poll is due. */
  private settleRead() {
    if (this.disposed) return;
    if (this.rereadSessions) {
      this.rereadSessions = false;
      this.refreshSessions();
    } else this.schedule();
  }
  /** A session that ended on its own may have been the last write: confirm live storage. */
  private applySessions(sessions: NativeSession[]) {
    const known = new Set(
      this.sessions.filter((s) => !isLive(s)).map((s) => s.id),
    );
    // An ending not seen before, whether it was live a moment ago or appeared already ended.
    const newlyEnded = sessions.some((s) => !isLive(s) && !known.has(s.id));
    this.sessions = sessions;
    if (newlyEnded) void this.refreshLauncher();
  }
  /**
   * The one way live storage readiness is read. Each read takes a number when it starts, and only the
   * newest number may change the launcher: an answer that started earlier can never bring back a
   * readiness that a later read or a failed operation withdrew. A failed newest read leaves M9
   * unconfirmed until a newer read answers. Only a current read may report an error, and
   * secondary read failures never replace the original mutation failure.
   */
  private readReadiness(): Promise<boolean> {
    if (this.disposed || this.mutations > 0) return Promise.resolve(false);
    const seq = ++this.launcherSeq;
    const startedConfirmed = !this.m9Uncertain;
    let read: Promise<Readiness>;
    try {
      read = this.client.readiness();
    } catch (error) {
      read = Promise.reject(error);
    }
    const result = read
      .then(
        (launcher) => {
          if (seq !== this.launcherSeq || this.disposed) return false;
          // A reply that outlived its freshness interval may have captured ready before
          // another window degraded the store. Confirm again after it completes.
          if (this.launcherPending?.slow && startedConfirmed) return false;
          this.readiness = launcher;
          if (this.launcher)
            this.launcher = {
              ...this.launcher,
              m9_native_status: launcher.m9_native_status,
            };
          this.m9Uncertain = false;
          if (this.failure?.code === 'readiness_busy') this.failure = null;
          this.emit();
          return true;
        },
        (error: unknown) => {
          if (seq !== this.launcherSeq || this.disposed) return false;
          this.m9Uncertain = true;
          this.readFailed(error);
          return false;
        },
      )
      .finally(() => {
        if (this.launcherPending?.seq === seq) {
          const reconfirm = this.launcherPending.slow && startedConfirmed;
          this.launcherPending = null;
          this.emit();
          if (
            reconfirm &&
            !this.disposed &&
            this.visible &&
            seq === this.launcherSeq &&
            this.mutations === 0
          )
            void this.refreshLauncher();
        }
      });
    this.launcherPending = { seq, slow: false };
    this.emit();
    return result;
  }
  /** Discovery is reserved for initial load and changes to the project catalogue. Its
   * storage snapshot never confirms readiness or overwrites a newer lightweight answer. */
  private async refreshCatalogue() {
    if (this.disposed) return;
    const seq = ++this.catalogueSeq;
    const launcher = await this.client.launcher();
    if (this.disposed || seq !== this.catalogueSeq) return;
    this.launcher = {
      ...launcher,
      m9_native_status: this.readiness?.m9_native_status ?? 'legacy_host',
    };
    this.emit();
  }
  async refreshLauncher() {
    if (this.disposed) return;
    const confirmed = await this.readReadiness();
    if (confirmed && !this.disposed && this.phase === 'failed') {
      // The first load may have failed before sessions/profiles were available. A valid
      // retry reloads those too, instead of leaving a permanently failed page.
      this.phase = 'idle';
      this.start0();
    }
  }
  private readFailed(error: unknown) {
    if (this.disposed) return;
    if (this.failure) this.emit();
    else this.fail(error);
  }
  /** The retry button coalesces double clicks, but may supersede a stalled confirmation. */
  checkLauncher() {
    if (
      this.disposed ||
      (this.launcherPending?.seq === this.launcherSeq &&
        !this.launcherPending.slow)
    )
      return;
    void this.refreshLauncher();
  }
  async refreshProfiles() {
    if (this.disposed) return;
    try {
      const profiles = await this.client.profiles();
      if (this.disposed) return;
      this.profiles = profiles;
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
      await this.refreshCatalogue();
      await this.refreshLauncher();
      return path;
    } catch (error) {
      this.fail(error);
      return null;
    }
  }
  preview(
    agent: AgentId,
    profile: string | null,
    arsenal: ArsenalStart | null = null,
  ): Promise<Preview> {
    return arsenal
      ? this.client.preview(agent, profile, arsenal)
      : this.client.preview(agent, profile);
  }

  /** Starts the agent; the window gets its terminal. One start at a time. */
  /** Whether the next Claude Code or Codex start gets the Wyss delegation tool. */
  private delegation = false;
  setDelegation(on: boolean) {
    this.delegation = on;
  }

  async startAgent(
    agent: AgentId,
    project: string,
    profile: string | null,
    brief: string | null = null,
    preview: Preview | null = null,
  ): Promise<boolean> {
    return (
      (await this.startReply(agent, project, profile, brief, preview, null)) !==
      null
    );
  }

  /**
   * Starts the agent with an Arsenal selection named, and returns the reply so the caller can
   * record what the terminal carries. The desktop rebuilds the exclusion; the window never sends it.
   */
  async startWithArsenal(
    agent: AgentId,
    project: string,
    profile: string | null,
    preview: Preview | null,
    arsenal: ArsenalStart | null,
  ): Promise<StartReply | null> {
    return this.startReply(agent, project, profile, null, preview, arsenal);
  }

  private async startReply(
    agent: AgentId,
    project: string,
    profile: string | null,
    brief: string | null,
    preview: Preview | null,
    arsenal: ArsenalStart | null,
  ): Promise<StartReply | null> {
    if (this.starting || this.disposed) return null;
    if (this.refuseM9(agent)) return null;
    this.starting = true;
    this.failure = null;
    this.beginMutation();
    this.emit();
    try {
      const delegation =
        this.delegation && (agent === 'claude_code' || agent === 'codex');
      const reply = delegation
        ? await this.client.start(
            agent,
            project,
            profile,
            brief,
            preview,
            arsenal,
            true,
          )
        : arsenal
          ? await this.client.start(
              agent,
              project,
              profile,
              brief,
              preview,
              arsenal,
            )
          : await this.client.start(agent, project, profile, brief, preview);
      if (this.disposed) return null;
      this.afterStart(reply);
      return reply;
    } catch (error) {
      this.storeFailed(error, START_UNRECORDED);
      return null;
    } finally {
      this.starting = false;
      this.emit();
      this.endMutation();
    }
  }
  private afterStart(reply: StartReply) {
    if (this.disposed) return;
    this.sessions = [
      ...this.sessions.filter((s) => s.id !== reply.session.id),
      reply.session,
    ];
    this.lastShown = shownKey(this.sessions);
    this.onstarted(reply);
  }
  async restart(id: string): Promise<boolean> {
    if (this.starting || this.disposed) return false;
    if (this.refuseM9(this.sessions.find((s) => s.id === id)?.agent_id)) {
      return false;
    }
    this.starting = true;
    this.failure = null;
    this.beginMutation();
    this.emit();
    try {
      const reply = await this.client.restart(id);
      if (this.disposed) return false;
      this.afterStart(reply);
      return true;
    } catch (error) {
      this.storeFailed(error, START_UNRECORDED);
      return false;
    } finally {
      this.starting = false;
      this.emit();
      this.endMutation();
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
    if (this.disposed) return;
    this.beginMutation();
    try {
      for (const session of this.sessions.filter(
        (s) => s.state !== 'running' && s.state !== 'requested',
      ))
        try {
          if (this.disposed) break;
          await this.client.forget(session.id);
        } catch (error) {
          this.fail(error);
          break;
        }
    } finally {
      this.endMutation();
    }
  }
  /** The brief for handing a session's work to another agent; the user reviews it. */
  async handoff(id: string): Promise<HandoffBrief | null> {
    try {
      return await this.client.prepareHandoff(id);
    } catch (error) {
      this.fail(error);
      return null;
    }
  }
  async saveBrief(text: string): Promise<boolean> {
    try {
      return await this.client.saveHandoff(text);
    } catch (error) {
      this.fail(error);
      return false;
    }
  }
  /**
   * Stops the process. The desktop answers once, and nothing is retried here. Whatever the
   * answer, the launcher and the sessions are read again, so the view shows the store as it is.
   */
  async stop(id: string) {
    if (this.disposed) return;
    this.beginMutation();
    try {
      await this.client.stop(id);
    } catch (error) {
      // The process ended and only its record failed, when the desktop says so.
      this.storeFailed(error, STOP_UNRECORDED);
    } finally {
      this.endMutation();
    }
  }
  async forget(id: string) {
    if (this.disposed) return;
    this.beginMutation();
    try {
      await this.client.forget(id);
    } catch (error) {
      this.fail(error);
    } finally {
      this.endMutation();
    }
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
      await this.refreshCatalogue();
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
        ? msg('Profile exported.')
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
        this.notice = msg(
          'Profile imported. It grants nothing: read what it asks for before using it.',
        );
        await this.refreshProfiles();
      }
    } catch (error) {
      this.fail(error);
    }
  }

  dispose() {
    this.disposed = true;
    this.launcherSeq++;
    this.sessionsSeq++;
    this.rereadSessions = false;
    this.launcherPending = null;
    this.stopTimer();
  }
}
