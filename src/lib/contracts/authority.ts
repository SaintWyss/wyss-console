/**
 * AuthorityView v1: the desktop's read-only projection of the external authority.
 * It is not a control contract: it names no operation and carries no identity.
 */
export type AuthorityConnection =
  'connecting' | 'synchronizing' | 'connected' | 'waiting' | 'stopped';
export type AuthorityFault =
  'absent' | 'lost' | 'incompatible' | 'malformed' | 'unsafe_path';
export type AgentDesiredState = 'running' | 'stopped';
export type AgentActualState =
  'stopped' | 'starting' | 'initializing' | 'ready' | 'stopping' | 'failed';

export interface AuthoritySummary {
  service_version: string | null;
  protocol_version: number;
  restarts: number;
  /** Canonical decimal sequence; null before the first committed event. */
  last_sequence: string | null;
  prepared_sessions: number;
  closed_sessions: number;
}
export interface AgentSummary {
  adapter: string;
  desired: AgentDesiredState;
  actual: AgentActualState;
  runtime_version: string | null;
  model_count: number;
}
export interface AuthorityView {
  schema_version: 1;
  revision: number;
  connection: AuthorityConnection;
  reason: AuthorityFault | null;
  authority: AuthoritySummary | null;
  agent: AgentSummary | null;
}

const CONNECTIONS: readonly AuthorityConnection[] = [
  'connecting',
  'synchronizing',
  'connected',
  'waiting',
  'stopped',
];
const FAULTS: readonly AuthorityFault[] = [
  'absent',
  'lost',
  'incompatible',
  'malformed',
  'unsafe_path',
];
const DESIRED: readonly AgentDesiredState[] = ['running', 'stopped'];
const ACTUAL: readonly AgentActualState[] = [
  'stopped',
  'starting',
  'initializing',
  'ready',
  'stopping',
  'failed',
];

function invalid(field: string): Error {
  return new Error(`authority_view: invalid_${field}`);
}
function record(
  value: unknown,
  keys: readonly string[],
  field: string,
): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw invalid(field);
  const names = Object.keys(value);
  if (names.length !== keys.length || !keys.every((k) => names.includes(k)))
    throw invalid(field);
  return value as Record<string, unknown>;
}
function count(value: unknown, field: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    throw invalid(field);
  return value;
}
function member<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    throw invalid(field);
  return value as T;
}
/** Short printable ASCII, or null: nothing else is ever rendered as a version. */
function versionText(value: unknown, field: string): string | null {
  if (value === null) return null;
  if (typeof value !== 'string' || !/^[\x20-\x7e]{1,64}$/.test(value))
    throw invalid(field);
  return value;
}

function parseAuthority(value: unknown): AuthoritySummary {
  const v = record(
    value,
    [
      'service_version',
      'protocol_version',
      'restarts',
      'last_sequence',
      'prepared_sessions',
      'closed_sessions',
    ],
    'authority',
  );
  if (
    v.last_sequence !== null &&
    (typeof v.last_sequence !== 'string' ||
      !/^[1-9][0-9]{0,19}$/.test(v.last_sequence))
  )
    throw invalid('last_sequence');
  return {
    service_version: versionText(v.service_version, 'service_version'),
    protocol_version: count(v.protocol_version, 'protocol_version'),
    restarts: count(v.restarts, 'restarts'),
    last_sequence: v.last_sequence,
    prepared_sessions: count(v.prepared_sessions, 'prepared_sessions'),
    closed_sessions: count(v.closed_sessions, 'closed_sessions'),
  };
}
function parseAgent(value: unknown): AgentSummary {
  const v = record(
    value,
    ['adapter', 'desired', 'actual', 'runtime_version', 'model_count'],
    'agent',
  );
  if (
    typeof v.adapter !== 'string' ||
    !/^[a-z][a-z0-9_-]{0,63}$/.test(v.adapter)
  )
    throw invalid('adapter');
  return {
    adapter: v.adapter,
    desired: member(v.desired, DESIRED, 'desired'),
    actual: member(v.actual, ACTUAL, 'actual'),
    runtime_version: versionText(v.runtime_version, 'runtime_version'),
    model_count: count(v.model_count, 'model_count'),
  };
}

/** Validates the whole projection. Anything unexpected is an error, never a guess. */
export function parseAuthorityView(value: unknown): AuthorityView {
  const v = record(
    value,
    [
      'schema_version',
      'revision',
      'connection',
      'reason',
      'authority',
      'agent',
    ],
    'view',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  const connection = member(v.connection, CONNECTIONS, 'connection');
  const reason = v.reason === null ? null : member(v.reason, FAULTS, 'reason');
  // A reason outlives its failure only until an authority answers again.
  if (
    (connection === 'waiting') !== (reason !== null) &&
    connection !== 'connecting'
  )
    throw invalid('reason');
  // Authority and agent facts exist only while the link is connected.
  const live = connection === 'connected';
  if ((v.authority !== null) !== live || (v.agent !== null) !== live)
    throw invalid('view');
  return {
    schema_version: 1,
    revision: count(v.revision, 'revision'),
    connection,
    reason,
    authority: live ? parseAuthority(v.authority) : null,
    agent: live ? parseAgent(v.agent) : null,
  };
}
