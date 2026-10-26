/**
 * Native sessions (F3): what the desktop says about the agents it started in terminals, the
 * profiles they start with, and why a start was refused. Closed words and counts; the window
 * checks every answer against this shape and believes nothing that does not fit.
 *
 * Nothing here is a command, an argument list or an environment: the page names an agent from
 * a closed list, a project the user chose, and a profile.
 */
import type { TerminalInfo } from './terminal';
import { parseTerminalId } from './identities';

export const AGENT_IDS = [
  'claude_code',
  'codex',
  'opencode',
  'hermes',
] as const;
export type AgentId = (typeof AGENT_IDS)[number];

export const SESSION_STATES = [
  'requested',
  'running',
  'exited',
  'stopped',
  'failed_start',
  'lost',
] as const;
export type SessionState = (typeof SESSION_STATES)[number];

export const PERMISSIONS = ['ask', 'read_only', 'edits'] as const;
export type Permission = (typeof PERMISSIONS)[number];
export const HOOKS_MODES = ['inherit', 'off'] as const;
export type HooksMode = (typeof HOOKS_MODES)[number];

export const PROFILE_CAPABILITY_STATUSES = [
  'syntax_verified',
  'unverified',
] as const;
export type ProfileCapabilityStatus =
  (typeof PROFILE_CAPABILITY_STATUSES)[number];
export interface ProfileCapability {
  status: ProfileCapabilityStatus;
  qualified_version: string | null;
  checked_on: string | null;
  operations: string[];
  interface: string | null;
  format: string | null;
  scope: string | null;
  reload_semantics: string | null;
  source: string | null;
  limitation: string;
}

const UNVERIFIED_PROFILE_CAPABILITY: ProfileCapability = {
  status: 'unverified',
  qualified_version: null,
  checked_on: null,
  operations: [],
  interface: null,
  format: null,
  scope: null,
  reload_semantics: null,
  source: null,
  limitation: 'No version-bound profile-override evidence was reported.',
};

export interface AgentEntry {
  agent_id: AgentId;
  name: string;
  executable: string;
  installed: boolean;
  path: string | null;
  version: string | null;
  /** Exact-build syntax evidence; it never proves provider resolution or effective use. */
  profile_capability: ProfileCapability;
}
export interface ShellInfo {
  path: string;
  family: string | null;
  through_shell: boolean;
}
export interface ProjectEntry {
  path: string;
  home: boolean;
  default_agent: AgentId | null;
  default_profile: string | null;
  last_used: number;
}
export interface Launcher {
  agents: AgentEntry[];
  shell: ShellInfo | null;
  projects: ProjectEntry[];
  m9_native_status: 'ready' | 'legacy_host' | 'storage_unavailable';
}
export interface Preview {
  applied: string[];
  with_cost: string[];
  not_applicable: string[];
  can_start: boolean;
  through_shell: boolean;
  warnings: string[];
  /** Opaque token for the exact profile plan reviewed in this preview. */
  plan_token: string | null;
  /** Exact `--version` result for the discovered CLI path at preview time. */
  provider_version: string | null;
}
export interface Usage {
  cpu_percent: number | null;
  rss_bytes: number;
  processes: number;
}
export interface ProfileRef {
  id: string;
  name: string;
  version: number;
}
export interface NativeSession {
  id: string;
  agent_id: AgentId;
  agent_name: string;
  version: string | null;
  project: string;
  profile: ProfileRef | null;
  through_shell: boolean;
  state: SessionState;
  started_at: number;
  ended_at: number | null;
  exit_code: number | null;
  exit_signal: string | null;
  error: string | null;
  restart_of: string | null;
  restarts: number;
  plane: 'native';
  terminal_id: string | null;
  pid: number | null;
  usage: Usage | null;
}
export interface StartReply {
  session: NativeSession;
  terminal: TerminalInfo;
}
export interface ProfileBody {
  name: string;
  applies_to: AgentId[];
  extends: string | null;
  model: string | null;
  /** A reasoning effort as the provider names it; passed, never checked here. */
  effort: string | null;
  permission: Permission | null;
  hooks: HooksMode | null;
  skills_off: string[];
  notes: string;
}
export interface ProfileVersion {
  n: number;
  saved_at: number;
  hash: string;
  name: string;
}
export interface Profile {
  id: string;
  template: boolean;
  current: number;
  versions: ProfileVersion[];
  body: ProfileBody;
  from: Record<string, string>;
  refers_to_missing: boolean;
}

export type NativeErrorCode = string;
export interface NativeFailure {
  code: NativeErrorCode;
  reasons: string[];
}

function bad(field: string): Error {
  return new Error(`native: invalid ${field}`);
}
function obj(value: unknown, field: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw bad(field);
  return value as Record<string, unknown>;
}
function str(value: unknown, field: string): string {
  if (typeof value !== 'string') throw bad(field);
  return value;
}
function num(value: unknown, field: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0)
    throw bad(field);
  return value;
}
function bool(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw bad(field);
  return value;
}
function arr(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) throw bad(field);
  return value;
}
function one<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T {
  if (
    typeof value !== 'string' ||
    !(allowed as readonly string[]).includes(value)
  )
    throw bad(field);
  return value as T;
}
function orNull<T>(value: unknown, read: (v: unknown) => T): T | null {
  return value === null ? null : read(value);
}
const strings = (value: unknown, field: string): string[] =>
  arr(value, field).map((v) => str(v, field));

function profileCapability(value: unknown): ProfileCapability {
  const o = obj(value, 'profile_capability');
  const capability: ProfileCapability = {
    status: one(
      o.status,
      PROFILE_CAPABILITY_STATUSES,
      'profile_capability.status',
    ),
    qualified_version: orNull(o.qualified_version, (v) =>
      str(v, 'profile_capability.qualified_version'),
    ),
    checked_on: orNull(o.checked_on, (v) =>
      str(v, 'profile_capability.checked_on'),
    ),
    operations:
      o.operations === undefined
        ? []
        : strings(o.operations, 'profile_capability.operations'),
    interface: orNull(o.interface, (v) =>
      str(v, 'profile_capability.interface'),
    ),
    format: orNull(o.format, (v) => str(v, 'profile_capability.format')),
    scope: orNull(o.scope, (v) => str(v, 'profile_capability.scope')),
    reload_semantics: orNull(o.reload_semantics, (v) =>
      str(v, 'profile_capability.reload_semantics'),
    ),
    source: orNull(o.source, (v) => str(v, 'profile_capability.source')),
    limitation: str(o.limitation, 'profile_capability.limitation'),
  };
  if (
    capability.status === 'syntax_verified' &&
    (!capability.qualified_version ||
      !capability.checked_on ||
      capability.operations.length === 0 ||
      !capability.interface ||
      !capability.format ||
      !capability.scope ||
      !capability.reload_semantics ||
      !capability.source)
  )
    throw bad('profile_capability.evidence');
  return capability;
}

function agent(value: unknown): AgentEntry {
  const o = obj(value, 'agent');
  return {
    agent_id: one(o.agent_id, AGENT_IDS, 'agent_id'),
    name: str(o.name, 'name'),
    executable: str(o.executable, 'executable'),
    installed: bool(o.installed, 'installed'),
    path: orNull(o.path, (v) => str(v, 'path')),
    version: orNull(o.version, (v) => str(v, 'version')),
    profile_capability:
      o.profile_capability === undefined
        ? { ...UNVERIFIED_PROFILE_CAPABILITY }
        : profileCapability(o.profile_capability),
  };
}
function project(value: unknown): ProjectEntry {
  const o = obj(value, 'project');
  return {
    path: str(o.path, 'path'),
    home: bool(o.home, 'home'),
    default_agent: orNull(o.default_agent, (v) => one(v, AGENT_IDS, 'agent')),
    default_profile: orNull(o.default_profile, (v) => str(v, 'profile')),
    last_used: num(o.last_used, 'last_used'),
  };
}
export function parseLauncher(value: unknown): Launcher {
  const o = obj(value, 'launcher');
  return {
    agents: arr(o.agents, 'agents').map(agent),
    shell: orNull(o.shell, (v) => {
      const s = obj(v, 'shell');
      return {
        path: str(s.path, 'path'),
        family: orNull(s.family, (f) => str(f, 'family')),
        through_shell: bool(s.through_shell, 'through_shell'),
      };
    }),
    projects: arr(o.projects, 'projects').map(project),
    // M1-M6 hosts predate the additive M9 status field and remain compatible.
    m9_native_status:
      o.m9_native_status === undefined
        ? 'legacy_host'
        : one(
            o.m9_native_status,
            ['ready', 'legacy_host', 'storage_unavailable'],
            'm9_native_status',
          ),
  };
}
export function parsePreview(value: unknown): Preview {
  const o = obj(value, 'preview');
  return {
    applied: strings(o.applied, 'applied'),
    with_cost: strings(o.with_cost, 'with_cost'),
    not_applicable: strings(o.not_applicable, 'not_applicable'),
    can_start: bool(o.can_start, 'can_start'),
    through_shell: bool(o.through_shell, 'through_shell'),
    warnings: o.warnings === undefined ? [] : strings(o.warnings, 'warnings'),
    plan_token:
      o.plan_token === undefined
        ? null
        : orNull(o.plan_token, (v) => str(v, 'plan_token')),
    provider_version:
      o.provider_version === undefined
        ? null
        : orNull(o.provider_version, (v) => str(v, 'provider_version')),
  };
}
export function parseSession(value: unknown): NativeSession {
  const o = obj(value, 'session');
  if (o.plane !== 'native') throw bad('plane');
  return {
    id: str(o.id, 'id'),
    agent_id: one(o.agent_id, AGENT_IDS, 'agent_id'),
    agent_name: str(o.agent_name, 'agent_name'),
    version: orNull(o.version, (v) => str(v, 'version')),
    project: str(o.project, 'project'),
    profile: orNull(o.profile, (v) => {
      const p = obj(v, 'profile');
      return {
        id: str(p.id, 'id'),
        name: str(p.name, 'name'),
        version: num(p.version, 'version'),
      };
    }),
    through_shell: bool(o.through_shell, 'through_shell'),
    state: one(o.state, SESSION_STATES, 'state'),
    started_at: num(o.started_at, 'started_at'),
    ended_at: orNull(o.ended_at, (v) => num(v, 'ended_at')),
    exit_code: orNull(o.exit_code, (v) => num(v, 'exit_code')),
    exit_signal: orNull(o.exit_signal, (v) => str(v, 'exit_signal')),
    error: orNull(o.error, (v) => str(v, 'error')),
    restart_of: orNull(o.restart_of, (v) => str(v, 'restart_of')),
    restarts: num(o.restarts, 'restarts'),
    plane: 'native',
    terminal_id: orNull(o.terminal_id, (v) => str(v, 'terminal_id')),
    pid: orNull(o.pid, (v) => num(v, 'pid')),
    usage: orNull(o.usage, (v) => {
      const u = obj(v, 'usage');
      return {
        cpu_percent: orNull(u.cpu_percent, (c) => num(c, 'cpu')),
        rss_bytes: num(u.rss_bytes, 'rss'),
        processes: num(u.processes, 'processes'),
      };
    }),
  };
}
export const parseSessions = (value: unknown): NativeSession[] =>
  arr(value, 'sessions').map(parseSession);

export function parseStartReply(value: unknown): StartReply {
  const o = obj(value, 'reply');
  const session = parseSession(o.session);
  const t = obj(o.terminal, 'terminal');
  // The terminal's own shape is checked by the terminal client when it adopts it; what
  // matters here is that it is the terminal the session names.
  if (session.terminal_id !== t.terminal_id) throw bad('terminal');
  parseTerminalId(t.terminal_id);
  return { session, terminal: o.terminal as TerminalInfo };
}

export function parseBody(value: unknown): ProfileBody {
  const o = obj(value, 'body');
  return {
    name: str(o.name, 'name'),
    applies_to: arr(o.applies_to, 'applies_to').map((v) =>
      one(v, AGENT_IDS, 'applies_to'),
    ),
    extends: orNull(o.extends, (v) => str(v, 'extends')),
    model: orNull(o.model, (v) => str(v, 'model')),
    // Absent from a profile saved before efforts existed, and from one that sets none.
    effort:
      o.effort === undefined ? null : orNull(o.effort, (v) => str(v, 'effort')),
    permission: orNull(o.permission, (v) => one(v, PERMISSIONS, 'permission')),
    hooks: orNull(o.hooks, (v) => one(v, HOOKS_MODES, 'hooks')),
    skills_off: strings(o.skills_off, 'skills_off'),
    notes: str(o.notes, 'notes'),
  };
}
export function parseProfiles(value: unknown): Profile[] {
  return arr(value, 'profiles').map((v) => {
    const o = obj(v, 'profile');
    const from = obj(o.from, 'from');
    return {
      id: str(o.id, 'id'),
      template: bool(o.template, 'template'),
      current: num(o.current, 'current'),
      versions: arr(o.versions, 'versions').map((x) => {
        const version = obj(x, 'version');
        return {
          n: num(version.n, 'n'),
          saved_at: num(version.saved_at, 'saved_at'),
          hash: str(version.hash, 'hash'),
          name: str(version.name, 'name'),
        };
      }),
      body: parseBody(o.body),
      from: Object.fromEntries(
        Object.entries(from).map(([k, name]) => [k, str(name, 'from')]),
      ),
      refers_to_missing: bool(o.refers_to_missing, 'refers_to_missing'),
    };
  });
}

export function parseFailure(error: unknown): NativeFailure {
  const o = typeof error === 'object' && error !== null ? error : {};
  const code = (o as { code?: unknown }).code;
  const reasons = (o as { reasons?: unknown }).reasons;
  return {
    code:
      typeof code === 'string' && /^[a-z_]{1,48}$/.test(code)
        ? code
        : 'internal',
    reasons: Array.isArray(reasons)
      ? reasons.filter((r): r is string => typeof r === 'string').slice(0, 8)
      : [],
  };
}
