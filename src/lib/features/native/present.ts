import type {
  AgentId,
  HooksMode,
  NativeSession,
  Permission,
  Profile,
  ProfileBody,
  SessionState,
} from '../../contracts/native';
import type { ConfigItem } from '../../contracts/configuration';

export type Tone = 'ok' | 'pending' | 'warn' | 'error' | 'idle';

export const AGENT_LABEL: Record<AgentId, string> = {
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  hermes: 'Hermes Agent (Native)',
};

/** The agents whose sessions the Console records in its M9 store; the others are legacy. */
export const isM9Agent = (agent: AgentId): boolean =>
  agent === 'opencode' || agent === 'hermes';

/** Shown when the process ended and only its record failed: the stop happened, the write did not. */
/** Shown when a start or restart failed while its record was written: a process may exist. */
export const START_UNRECORDED =
  'The Console could not record this start, so a process may exist. Check the sessions below. Nothing is started again automatically, and OpenCode and Hermes wait until the store is confirmed.';

/** Shown when a stop ended the process and only its record failed. */
export const STOP_UNRECORDED =
  'The process was stopped, but the Console could not record that. Nothing was started again, and OpenCode and Hermes stay blocked until the store is confirmed.';

/** A restart's new session is "restarting" until its process runs. */
export function stateLabel(s: NativeSession): { text: string; tone: Tone } {
  switch (s.state) {
    case 'requested':
      return s.restart_of
        ? { text: 'Restarting', tone: 'pending' }
        : { text: 'Starting', tone: 'pending' };
    case 'running':
      return { text: 'Active', tone: 'ok' };
    case 'exited':
      if (s.exit_signal) return { text: 'Ended by a signal', tone: 'error' };
      return s.exit_code === 0
        ? { text: 'Exited (0)', tone: 'idle' }
        : { text: `Failed (${s.exit_code ?? '?'})`, tone: 'error' };
    case 'stopped':
      return { text: 'Stopped', tone: 'warn' };
    case 'failed_start':
      return { text: 'Did not start', tone: 'error' };
    case 'lost':
      return { text: 'Lost', tone: 'warn' };
  }
}

export const STATE_NOTE: Record<SessionState, string> = {
  requested: 'The start was accepted and is being prepared.',
  running: 'The agent’s process is alive in its terminal.',
  exited: 'The process ended by itself.',
  stopped: 'You stopped it.',
  failed_start: 'The process never ran.',
  lost: 'The Console was restarted and did not see how it ended. Its terminal went with the window, so it is not running.',
};

export const FAILURE_TEXT: Record<string, string> = {
  agent_not_installed: 'That agent is not installed on this computer.',
  project_invalid:
    'That folder cannot be used. Choose a folder with the folder button.',
  profile_not_applicable: 'This profile cannot be applied to that agent.',
  profile_unknown: 'That profile no longer exists.',
  profile_preview_required:
    'Review the current profile preview before starting with it.',
  profile_preview_stale:
    'The profile changed after its preview. Review the new preview and start again.',
  provider_version_unverified:
    'This exact provider build is not verified for profile overrides, so the override was not launched. Choose “No profile — as configured” or use a qualified build.',
  arsenal_unqualified:
    'Skill exclusion is not qualified for this exact provider build, so nothing was started with it. Start without applying the selection, or use a qualified build.',
  arsenal_plan_stale:
    'The Arsenal plan changed since it was shown: the library, the installed skills or the provider build moved. Review the new plan and start again.',
  arsenal_blocked:
    'The Arsenal plan has items that cannot be carried or names a skill that is not installed, so nothing was started with it.',
  arsenal_unavailable:
    'The Arsenal library or the installed skills could not be read, so nothing was started with the selection.',
  arsenal_selection_unknown: 'That Arsenal selection no longer exists.',
  arsenal_project_refused:
    'Arsenal does not read this project folder, so its selection cannot be applied here.',
  handoff_target_unverified:
    'Hermes Native does not have a verified interactive handoff argument. It was not started with this brief.',
  brief_invalid:
    'The brief must start with a “# ” heading, hold no NUL and be at most 16 KiB.',
  limit_reached:
    'The terminal limit (16) is reached. Close one to start another.',
  spawn_failed: 'The agent could not be started.',
  busy: 'A start is already in progress.',
  session_unknown: 'That session is not known any more.',
  not_running: 'That session is not running.',
  still_running: 'A running session cannot be removed. Stop it first.',
  stop_failed: 'The session could not be stopped.',
  store_unavailable: 'The Console could not write its own state.',
  readiness_busy:
    'Session storage is busy. Readiness could not be checked; it will be retried.',
  unavailable:
    'The Console has no private folder to keep native sessions in, so it cannot start agents.',
  request_invalid: 'The request was not valid.',
  host_outdated:
    'The terminal host is older than this window and cannot keep these profile options. Update it before using effort or preview-bound profiles; existing sessions are unchanged.',
  export_failed: 'The file could not be written.',
  profile_name_invalid:
    'The name must be 1 to 60 characters, without control characters.',
  profile_applies_to_invalid: 'Choose at least one agent, each once.',
  profile_model_invalid:
    'The model may hold letters, digits and . _ : - / only, and may not start with a dash.',
  profile_effort_invalid:
    'The effort may hold letters, digits and . _ - only (32 at most), and may not start with a dash.',
  profile_skill_invalid:
    'A skill name may hold letters, digits and . _ : - only (64 at most).',
  profile_notes_invalid: 'The notes are too long or hold control characters.',
  profile_notes_hold_secret:
    'The text looks like it holds a secret. A profile never holds one.',
  profile_extends_unknown: 'The profile it extends does not exist.',
  profile_profile_cycle:
    'A profile cannot extend itself, directly or through others.',
  profile_too_deep: 'Profiles may extend up to eight levels.',
  profile_read_only_template:
    'A template is read only. Make a copy to change it.',
  profile_too_many: 'There are too many profiles (100).',
  profile_version_unknown: 'That version does not exist.',
  profile_import_invalid: 'That file is not a profile this app can read.',
  profile_store_unavailable: 'The profiles could not be read or written.',
  profile_in_use: 'Another profile extends this one. Change that one first.',
  internal: 'Something went wrong in the desktop.',
};
export const failureText = (code: string) =>
  FAILURE_TEXT[code] ?? 'Something went wrong in the desktop.';

export const PERMISSION_LABEL: Record<Permission, string> = {
  ask: 'Ask before acting',
  read_only: 'Read only (plan)',
  edits: 'Accept edits (Claude Code only)',
};
export const HOOKS_LABEL: Record<HooksMode, string> = {
  inherit: 'As configured',
  off: 'Off for this session (Claude Code only)',
};

export function folderName(path: string): string {
  return path.split('/').filter(Boolean).pop() ?? '/';
}
export function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}
export function since(from: number, now: number): string {
  const seconds = Math.max(0, Math.round(now / 1000 - from));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400)
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  return `${Math.floor(seconds / 86400)}d`;
}

export function emptyBody(): ProfileBody {
  return {
    name: '',
    applies_to: ['claude_code', 'codex'],
    extends: null,
    model: null,
    effort: null,
    permission: null,
    hooks: null,
    skills_off: [],
    notes: '',
  };
}

/** Profiles that can be chosen to start `agent`: written for it. */
/**
 * The three things said about the model of a native session, kept apart because they are
 * different facts: what the session's profile asked for, what the agent's own files are set
 * to, and what is in use. The last is never filled from the other two: neither agent reports
 * the model it runs without a hook or a read of the terminal, and this app adds neither.
 */
export interface ModelFacts {
  requested: string;
  /** The effort the profile asked for, or why there is none to say. */
  requestedEffort: string;
  configured: string;
  /** The effort the agent's own files hold, or why there is none to say. */
  configuredEffort: string;
  inUse: string;
}
export function modelFacts(
  session: NativeSession,
  profiles: Profile[],
  config: readonly ConfigItem[] | null,
): ModelFacts {
  let requested = 'none (the agent’s own choice)';
  let requestedEffort = requested;
  if (session.profile) {
    const profile = profiles.find((p) => p.id === session.profile?.id);
    if (!profile) requested = requestedEffort = 'the profile no longer exists';
    else if (profile.current !== session.profile.version)
      requested = requestedEffort =
        'the profile has changed since this session started';
    else {
      if (profile.body.model) requested = profile.body.model;
      if (profile.body.effort) requestedEffort = profile.body.effort;
    }
  }
  let configured = 'not read yet (open Configuration)';
  let configuredEffort = configured;
  if (config !== null) {
    const provider =
      session.agent_id === 'claude_code'
        ? 'claude_code'
        : session.agent_id === 'codex'
          ? 'codex'
          : null;
    const keyed = (key: string): string | null => {
      const item = config.find(
        (i) =>
          i.provider === provider &&
          i.kind === 'model_preference' &&
          i.name === key &&
          i.scope === 'global' &&
          i.detail?.kind === 'text',
      );
      return item?.detail?.kind === 'text' ? item.detail.value.text : null;
    };
    // Each agent names its effort key its own way.
    if (provider) {
      const effortKey =
        session.agent_id === 'claude_code'
          ? 'effortLevel'
          : 'model_reasoning_effort';
      configured = keyed('model') ?? 'not set in its files';
      configuredEffort = keyed(effortKey) ?? 'not set in its files';
    } else {
      configured = configuredEffort =
        'not observed by this provider configuration view';
    }
  }
  return {
    requested,
    requestedEffort,
    configured,
    configuredEffort,
    inUse: 'not reported by the agent',
  };
}

/**
 * What can be said about changing the model of a session that is already running. Claude
 * Code's own command is documented (and says it saves your default too); Codex's is not
 * verified, so none is offered. The text sends nothing to the terminal and copies nothing.
 */
export function switchHint(agent: AgentId): string {
  switch (agent) {
    case 'claude_code':
      return 'To change the model in this session, type /model in it. Claude Code also saves your choice as its default.';
    case 'codex':
      return 'How to change the model of a running Codex session is not verified, so none is suggested.';
    case 'opencode':
    case 'hermes':
      return 'How to change the model in a running session is not verified, so none is suggested.';
  }
}

export function profilesFor(profiles: Profile[], agent: AgentId): Profile[] {
  return profiles.filter((p) => p.body.applies_to.includes(agent));
}

/** A preview is current only for the selected profile and every revision it inherits. */
export function profileRevisionKey(
  profiles: readonly Profile[],
  id: string | null,
): string {
  if (!id) return '';
  const byId = new Map(profiles.map((profile) => [profile.id, profile]));
  const revisions: unknown[] = [];
  const seen = new Set<string>();
  let next: string | null = id;
  while (next && revisions.length < 8) {
    if (seen.has(next)) {
      revisions.push([next, 'cycle']);
      next = null;
      break;
    }
    seen.add(next);
    const profile = byId.get(next);
    if (!profile) {
      revisions.push([next, 'missing']);
      next = null;
      break;
    }
    const version = profile.versions.find((item) => item.n === profile.current);
    revisions.push([
      profile.id,
      profile.current,
      version?.hash ?? null,
      profile.body.extends,
    ]);
    next = profile.body.extends;
  }
  if (next) revisions.push([next, 'depth']);
  return JSON.stringify(revisions);
}

/** Sessions of one project, then by start: grouping for the overview (FR-NAT-08). */
export function groupByProject(
  sessions: NativeSession[],
): { project: string; sessions: NativeSession[] }[] {
  const groups = new Map<string, NativeSession[]>();
  for (const s of sessions)
    groups.set(s.project, [...(groups.get(s.project) ?? []), s]);
  return [...groups.entries()]
    .map(([project, list]) => ({
      project,
      sessions: list.sort((a, b) => b.started_at - a.started_at),
    }))
    .sort(
      (a, b) =>
        Math.max(...b.sessions.map((s) => s.started_at)) -
        Math.max(...a.sessions.map((s) => s.started_at)),
    );
}

/** Two running sessions on one folder can write the same tree (09 §6). */
export function sharedFolders(sessions: NativeSession[]): Set<string> {
  const running = new Map<string, number>();
  for (const s of sessions)
    if (s.state === 'running')
      running.set(s.project, (running.get(s.project) ?? 0) + 1);
  return new Set([...running].filter(([, n]) => n > 1).map(([p]) => p));
}

/**
 * Sessions the Console lost sight of and has not started again since: what a restored
 * workspace would start. A record that already has a successor is not offered twice.
 */
export function restorable(sessions: NativeSession[]): NativeSession[] {
  const restarted = new Set(
    sessions.map((s) => s.restart_of).filter((id): id is string => id !== null),
  );
  return sessions.filter((s) => s.state === 'lost' && !restarted.has(s.id));
}
