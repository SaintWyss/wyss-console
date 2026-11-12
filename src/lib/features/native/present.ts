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
import { msg } from '../../i18n/msg';

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
export const START_UNRECORDED = msg(
  'The Console could not record this start, so a process may exist. Check the sessions below. Nothing is started again automatically, and OpenCode and Hermes wait until the store is confirmed.',
);

/** Shown when a stop ended the process and only its record failed. */
export const STOP_UNRECORDED = msg(
  'The process was stopped, but the Console could not record that. Nothing was started again, and OpenCode and Hermes stay blocked until the store is confirmed.',
);

/** A restart's new session is "restarting" until its process runs. */
export function stateLabel(s: NativeSession): { text: string; tone: Tone } {
  switch (s.state) {
    case 'requested':
      return s.restart_of
        ? { text: msg('Restarting'), tone: 'pending' }
        : { text: msg('Starting'), tone: 'pending' };
    case 'running':
      return { text: msg('Active'), tone: 'ok' };
    case 'exited':
      if (s.exit_signal)
        return { text: msg('Ended by a signal'), tone: 'error' };
      return s.exit_code === 0
        ? { text: msg('Exited (0)'), tone: 'idle' }
        : { text: `Failed (${s.exit_code ?? '?'})`, tone: 'error' };
    case 'stopped':
      return { text: msg('Stopped'), tone: 'warn' };
    case 'failed_start':
      return { text: msg('Did not start'), tone: 'error' };
    case 'lost':
      return { text: msg('Lost'), tone: 'warn' };
  }
}

export const STATE_NOTE: Record<SessionState, string> = {
  requested: msg('The start was accepted and is being prepared.'),
  running: msg('The agent’s process is alive in its terminal.'),
  exited: msg('The process ended by itself.'),
  stopped: msg('You stopped it.'),
  failed_start: msg('The process never ran.'),
  lost: msg(
    'The Console was restarted and did not see how it ended. Its terminal went with the window, so it is not running.',
  ),
};

export const FAILURE_TEXT: Record<string, string> = {
  agent_not_installed: msg('That agent is not installed on this computer.'),
  project_invalid: msg(
    'That folder cannot be used. Choose a folder with the folder button.',
  ),
  profile_not_applicable: msg('This profile cannot be applied to that agent.'),
  profile_unknown: msg('That profile no longer exists.'),
  profile_preview_required: msg(
    'Review the current profile preview before starting with it.',
  ),
  profile_preview_stale: msg(
    'The profile changed after its preview. Review the new preview and start again.',
  ),
  provider_version_unverified: msg(
    'This exact provider build is not verified for profile overrides, so the override was not launched. Choose “No profile — as configured” or use a qualified build.',
  ),
  arsenal_unqualified: msg(
    'Skill exclusion is not qualified for this exact provider build, so nothing was started with it. Start without applying the selection, or use a qualified build.',
  ),
  arsenal_plan_stale: msg(
    'The Arsenal plan changed since it was shown: the library, the installed skills or the provider build moved. Review the new plan and start again.',
  ),
  arsenal_blocked: msg(
    'The Arsenal plan has items that cannot be carried or names a skill that is not installed, so nothing was started with it.',
  ),
  arsenal_unavailable: msg(
    'The Arsenal library or the installed skills could not be read, so nothing was started with the selection.',
  ),
  arsenal_selection_unknown: msg('That Arsenal selection no longer exists.'),
  arsenal_project_refused: msg(
    'Arsenal does not read this project folder, so its selection cannot be applied here.',
  ),
  handoff_target_unverified: msg(
    'Hermes Native does not have a verified interactive handoff argument. It was not started with this brief.',
  ),
  brief_invalid: msg(
    'The brief must start with a “# ” heading, hold no NUL and be at most 16 KiB.',
  ),
  limit_reached: msg(
    'The terminal limit (16) is reached. Close one to start another.',
  ),
  spawn_failed: msg('The agent could not be started.'),
  busy: msg('A start is already in progress.'),
  session_unknown: msg('That session is not known any more.'),
  not_running: msg('That session is not running.'),
  still_running: msg('A running session cannot be removed. Stop it first.'),
  stop_failed: msg('The session could not be stopped.'),
  store_unavailable: msg('The Console could not write its own state.'),
  readiness_busy: msg(
    'Session storage is busy. Readiness could not be checked; it will be retried.',
  ),
  unavailable: msg(
    'The Console has no private folder to keep native sessions in, so it cannot start agents.',
  ),
  request_invalid: msg('The request was not valid.'),
  host_unavailable: msg(
    'The terminal host is not answering. Nothing was sent; nothing changed.',
  ),
  host_outcome_unknown: msg(
    'The terminal host did not confirm this request: it may or may not have taken effect, and it was not sent again. Check the sessions listed here before repeating it, so nothing runs twice.',
  ),
  host_outdated: msg(
    'The terminal host is older than this window and cannot keep these profile options. Update it before using effort or preview-bound profiles; existing sessions are unchanged.',
  ),
  export_failed: msg('The file could not be written.'),
  profile_name_invalid: msg(
    'The name must be 1 to 60 characters, without control characters.',
  ),
  profile_applies_to_invalid: msg('Choose at least one agent, each once.'),
  profile_model_invalid: msg(
    'The model may hold letters, digits and . _ : - / only, and may not start with a dash.',
  ),
  profile_effort_invalid: msg(
    'The effort may hold letters, digits and . _ - only (32 at most), and may not start with a dash.',
  ),
  profile_skill_invalid: msg(
    'A skill name may hold letters, digits and . _ : - only (64 at most).',
  ),
  profile_notes_invalid: msg(
    'The notes are too long or hold control characters.',
  ),
  profile_notes_hold_secret: msg(
    'The text looks like it holds a secret. A profile never holds one.',
  ),
  profile_extends_unknown: msg('The profile it extends does not exist.'),
  profile_profile_cycle: msg(
    'A profile cannot extend itself, directly or through others.',
  ),
  profile_too_deep: msg('Profiles may extend up to eight levels.'),
  profile_read_only_template: msg(
    'A template is read only. Make a copy to change it.',
  ),
  profile_too_many: msg('There are too many profiles (100).'),
  profile_version_unknown: msg('That version does not exist.'),
  profile_import_invalid: msg('That file is not a profile this app can read.'),
  profile_store_unavailable: msg('The profiles could not be read or written.'),
  profile_in_use: msg(
    'Another profile extends this one. Change that one first.',
  ),
  internal: msg('Something went wrong in the desktop.'),
};
export const failureText = (code: string) =>
  FAILURE_TEXT[code] ?? msg('Something went wrong in the desktop.');

export const PERMISSION_LABEL: Record<Permission, string> = {
  ask: msg('Ask before acting'),
  read_only: msg('Read only (plan)'),
  edits: msg('Accept edits (Claude Code only)'),
  bypass: msg('Full access: no prompts, no sandbox (bypass)'),
};
/** Said wherever full access is chosen or shown: what it costs. */
export const BYPASS_WARNING = msg(
  'Full access: the agent asks nothing and runs every command and edit without a sandbox, with your account’s rights.',
);
export const HOOKS_LABEL: Record<HooksMode, string> = {
  inherit: msg('As configured'),
  off: msg('Off for this session (Claude Code only)'),
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
      return msg(
        'To change the model in this session, type /model in it. Claude Code also saves your choice as its default.',
      );
    case 'codex':
      return msg(
        'How to change the model of a running Codex session is not verified, so none is suggested.',
      );
    case 'opencode':
    case 'hermes':
      return msg(
        'How to change the model in a running session is not verified, so none is suggested.',
      );
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
