import type {
  AgentId,
  HooksMode,
  NativeSession,
  Permission,
  Profile,
  ProfileBody,
  SessionState,
} from '../../contracts/native';

export type Tone = 'ok' | 'pending' | 'warn' | 'error' | 'idle';

export const AGENT_LABEL: Record<AgentId, string> = {
  claude_code: 'Claude Code',
  codex: 'Codex',
};

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
  limit_reached:
    'The terminal limit (16) is reached. Close one to start another.',
  spawn_failed: 'The agent could not be started.',
  busy: 'A start is already in progress.',
  session_unknown: 'That session is not known any more.',
  not_running: 'That session is not running.',
  still_running: 'A running session cannot be removed. Stop it first.',
  stop_failed: 'The session could not be stopped.',
  store_unavailable: 'The Console could not write its own state.',
  unavailable:
    'The Console has no private folder to keep native sessions in, so it cannot start agents.',
  request_invalid: 'The request was not valid.',
  export_failed: 'The file could not be written.',
  profile_name_invalid:
    'The name must be 1 to 60 characters, without control characters.',
  profile_applies_to_invalid: 'Choose at least one agent, each once.',
  profile_model_invalid:
    'The model may hold letters, digits and . _ : - / only, and may not start with a dash.',
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
    permission: null,
    hooks: null,
    skills_off: [],
    notes: '',
  };
}

/** Profiles that can be chosen to start `agent`: written for it. */
export function profilesFor(profiles: Profile[], agent: AgentId): Profile[] {
  return profiles.filter((p) => p.body.applies_to.includes(agent));
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
