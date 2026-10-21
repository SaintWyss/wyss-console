/**
 * The desktop's projection of managed runtimes of protocol 2 (F8): schema 1 of every
 * payload below. It is not the control contract. It names no control method and carries no
 * process, no provider identity and no path of the runtime. Every payload names the
 * authority instance it was read from, so a window that sees the instance change drops
 * every fence, cursor and handle it holds.
 *
 * A fence is the expected observation a mutation is asked against: the generation and the
 * instance the window saw. It grants nothing; the authority compares it with the launch it
 * holds and refuses a stale one.
 */
import {
  parseAgentRuntimeInstanceId,
  parseManagedSessionId,
  type AgentRuntimeInstanceId,
  type ManagedSessionId,
} from './identities';
import {
  parseApprovalDetail,
  APPROVAL_STATES,
  type ApprovalDetail,
  type ApprovalState,
} from './approvals';
import { parseEventsPage, type EventsPage } from './events';
import { parseCatalogueEntry, type CatalogueEntry } from './models';
import { parseObservedModel, type ObservedModel } from './sessions';
import {
  parseApprovalNotice,
  parseTurnEvent,
  parseTurnSummary,
  turnEnded,
  type ApprovalNotice,
  type TurnEvent,
  type TurnSummary,
} from './turns';
import { parseUuid, type Uuid } from './uuid';

export const F8_FAILURES = [
  'inactive',
  'unavailable',
  'incompatible',
  'invalid',
  'not_found',
  'stale',
  'not_allowed',
  'not_bound',
  'runtime_unavailable',
  'turn_active',
  'workspace_invalid',
  'already_bound',
  'permission_denied',
  'lease_active',
  'busy',
  'rejected',
  'uncertain',
  'conflict',
  'internal',
] as const;
export type F8FailureCode = (typeof F8_FAILURES)[number];
export interface F8Failure {
  code: F8FailureCode;
  /** The outcome is unknown or nothing was refused: the same intent may be sent again. */
  retry: boolean;
  /** While the link waits to try again: how long until it does. */
  retry_in_ms: number | null;
}

export const LINK_STATES = [
  'connecting',
  'synchronizing',
  'connected',
  'waiting',
  'stopped',
] as const;
export type LinkStateName = (typeof LINK_STATES)[number];
export const LINK_FAULTS = [
  'absent',
  'lost',
  'incompatible',
  'malformed',
  'unsafe_path',
] as const;
export type LinkFault = (typeof LINK_FAULTS)[number];

export const DESIRED_STATES = ['running', 'stopped'] as const;
export type DesiredState = (typeof DESIRED_STATES)[number];
export const ACTUAL_STATES = [
  'stopped',
  'starting',
  'initializing',
  'ready',
  'stopping',
  'failed',
] as const;
export type ActualState = (typeof ACTUAL_STATES)[number];
export const RUNTIME_PROFILES = [
  'codex_inference_v1',
  'codex_local_tools_v1',
] as const;
export type RuntimeProfile = (typeof RUNTIME_PROFILES)[number];
export const RUNTIME_DIAGNOSTICS = [
  'unavailable',
  'spawn_failed',
  'transport_failed',
  'handshake_timeout',
  'protocol_failed',
  'server_request',
  'runtime_rejected',
  'catalog_rejected',
  'child_exited',
  'audit_unavailable',
  'inference_isolation_violation',
  'safety_unproven',
] as const;
export type RuntimeDiagnostic = (typeof RUNTIME_DIAGNOSTICS)[number];
export const SESSION_STATES = ['prepared', 'closed'] as const;
export type F8SessionState = (typeof SESSION_STATES)[number];
export const BINDING_STATES = [
  'unbound',
  'creating',
  'indeterminate',
  'bound_detached',
  'bound_attached',
] as const;
export type BindingStateName = (typeof BINDING_STATES)[number];
export const ATTACHMENT_PROFILES = [
  'inference_only_v1',
  'local_tools_v1',
] as const;
export type AttachmentProfile = (typeof ATTACHMENT_PROFILES)[number];
export const LEASE_SCOPES = [
  'session_create',
  'session_resume',
  'turn_start',
  'command_execute',
  'file_change',
] as const;
export type LeaseScope = (typeof LEASE_SCOPES)[number];
export const LEASE_STATES = ['active', 'revoked', 'expired'] as const;
export type LeaseStateName = (typeof LEASE_STATES)[number];
export const REVOCATIONS = ['user_request', 'session_closed'] as const;
export const GATE_REASONS = [
  'allowed',
  'binding_not_found',
  'binding_detached',
  'attachment_runtime_mismatch',
  'inference_profile_missing',
  'local_tools_profile_missing',
  'runtime_safety_profile_missing',
  'active_turn_exists',
  'session_not_found',
  'session_not_prepared',
  'lease_missing',
  'lease_not_active',
  'lease_expired',
  'scope_not_granted',
  'runtime_mismatch',
  'runtime_not_running',
  'runtime_not_ready',
  'runtime_identity_missing',
  'runtime_incompatible',
  'authority_unavailable',
] as const;
export type GateReasonName = (typeof GATE_REASONS)[number];
export const GATE_ACTIONS = [
  'turn_start',
  'command_execute',
  'file_change',
] as const;

/** Lease lifetimes the window offers, in seconds. The protocol allows one to 3600. */
export const LEASE_TTLS = [60, 300, 900, 1800, 3600] as const;
export const SESSION_PAGE_LIMIT = 25;
export const TURN_PAGE_LIMIT = 8;
export const TURN_EVENTS_LIMIT = 50;

export interface AuthorityInfo {
  instance_id: Uuid;
  service_version: string;
  protocol_version: 2;
  storage_schema_version: 8;
  stream_id: Uuid;
  last_sequence: string | null;
  prepared_count: number;
  closed_count: number;
}
export interface LinkInfo {
  state: LinkStateName;
  fault: LinkFault | null;
  retry_in_ms: number | null;
}
export interface F8State {
  schema_version: 1;
  active: boolean;
  link: LinkInfo;
  authority: AuthorityInfo | null;
  epoch: number;
}

export interface Fence {
  session: ManagedSessionId;
  generation: number;
  instance: AgentRuntimeInstanceId | null;
}
export interface RuntimeInfo {
  desired: DesiredState;
  actual: ActualState;
  revision: number;
  generation: number;
  instance: AgentRuntimeInstanceId | null;
  version: string | null;
  profile: RuntimeProfile | null;
  workspace: string | null;
  model_count: number;
  diagnostic: RuntimeDiagnostic | null;
  budget_failure: string | null;
  fence: Fence;
}
export interface SessionRow {
  id: ManagedSessionId;
  state: F8SessionState;
  revision: number;
  created_at: string;
  updated_at: string;
  position: string;
  runtime: RuntimeInfo;
}
export interface SessionsView {
  schema_version: 1;
  authority: Uuid;
  head: string | null;
  sessions: SessionRow[];
  older: string | null;
  capacity: number;
}
export interface BindingInfo {
  state: BindingStateName;
  workspace: string | null;
  mode: AttachmentProfile | null;
  operation: Uuid | null;
  observed_model: ObservedModel | null;
  attached_instance: AgentRuntimeInstanceId | null;
}
export interface LeaseInfo {
  id: Uuid;
  state: LeaseStateName;
  scopes: LeaseScope[];
  revision: number;
  issued_at: string;
  expires_at: string;
  revocation: 'user_request' | 'session_closed' | null;
}
export interface GateInfo {
  action: (typeof GATE_ACTIONS)[number];
  reason: GateReasonName;
}
export interface TurnInfo {
  turn: TurnSummary;
  /** The generation it ran on. Null for a turn from before protocol 2. */
  generation: number | null;
  /** The launch it ran on is the one that is attested and live now. */
  live: boolean;
}
export interface SessionDetail {
  schema_version: 1;
  authority: Uuid;
  session: SessionRow;
  binding: BindingInfo;
  lease: LeaseInfo | null;
  gates: GateInfo[];
  turns: TurnInfo[];
  turns_partial: boolean;
  approval: ApprovalDetail | null;
}
export interface TurnReading {
  schema_version: 1;
  authority: Uuid;
  turn: TurnInfo;
  events: TurnEvent[];
  next: string | null;
  more: boolean;
  gap: boolean;
  approval: ApprovalNotice | null;
}
export interface SessionReceipt {
  schema_version: 1;
  authority: Uuid;
  id: ManagedSessionId;
}
export interface TurnReceipt {
  schema_version: 1;
  authority: Uuid;
  id: Uuid;
}
export interface LeaseReceipt {
  schema_version: 1;
  authority: Uuid;
  lease: LeaseInfo;
}
export interface RuntimeReceipt {
  schema_version: 1;
  authority: Uuid;
  session: ManagedSessionId;
  desired: DesiredState;
  revision: number;
  generation: number;
}
export interface BindingReceipt {
  schema_version: 1;
  authority: Uuid;
  session: ManagedSessionId;
  workspace: string;
}
export interface AbandonReceipt {
  schema_version: 1;
  authority: Uuid;
  session: ManagedSessionId;
}
export interface DecisionReceipt {
  schema_version: 1;
  authority: Uuid;
  id: Uuid;
  state: ApprovalState;
}
export interface ModelsInfo {
  schema_version: 1;
  authority: Uuid;
  fence: Fence;
  version: string;
  complete: boolean | null;
  entries: CatalogueEntry[];
  observed: ObservedModel | null;
}
export interface AuditInfo {
  schema_version: 1;
  authority: Uuid;
  page: EventsPage;
}
export interface WatchInfo {
  schema_version: 1;
  authority: Uuid;
  stream: Uuid;
  head: string | null;
  changed: boolean;
}

// ---------------------------------------------------------------------------- parsing

const SEQUENCE = /^[1-9][0-9]{0,19}$/;
const TIMESTAMP =
  /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d{1,9})?([Zz]|\+00:00)$/;
const VERSION = /^[0-9]+(\.[0-9]+)*$/;
const BUDGET_KEY = /^[a-z][a-z0-9_]{0,63}$/;
/** u63: what the protocol counts generations and revisions in. */
const MAX_COUNT = Number.MAX_SAFE_INTEGER;

function invalid(field: string): Error {
  return new Error(`f8: invalid_${field}`);
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
function member<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    throw invalid(field);
  return value as T;
}
function flag(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw invalid(field);
  return value;
}
function count(value: unknown, field: string, least = 0): number {
  if (
    !Number.isSafeInteger(value) ||
    (value as number) < least ||
    (value as number) > MAX_COUNT
  )
    throw invalid(field);
  return value as number;
}
function sequence(value: unknown, field: string): string {
  if (typeof value !== 'string' || !SEQUENCE.test(value)) throw invalid(field);
  return value;
}
function timestamp(value: unknown, field: string): string {
  if (
    typeof value !== 'string' ||
    !TIMESTAMP.test(value) ||
    Number.isNaN(Date.parse(value))
  )
    throw invalid(field);
  return value;
}
function uuid(value: unknown, field: string): Uuid {
  try {
    return parseUuid(value);
  } catch {
    throw invalid(field);
  }
}
function sessionId(value: unknown): ManagedSessionId {
  try {
    return parseManagedSessionId(value);
  } catch {
    throw invalid('session');
  }
}
function instanceId(value: unknown): AgentRuntimeInstanceId | null {
  if (value === null) return null;
  try {
    return parseAgentRuntimeInstanceId(value);
  } catch {
    throw invalid('instance');
  }
}
function nullable<T>(value: unknown, parse: (v: unknown) => T): T | null {
  return value === null ? null : parse(value);
}
/** An absolute path of bounded length with no control character, as the authority keeps it. */
function folder(value: unknown): string {
  if (
    typeof value !== 'string' ||
    !value.startsWith('/') ||
    new TextEncoder().encode(value).length > 256 ||
    // eslint-disable-next-line no-control-regex
    /[\u0000-\u001f\u007f-\u009f]/.test(value)
  )
    throw invalid('workspace');
  return value;
}
function same(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}
function array(value: unknown, most: number, field: string): unknown[] {
  if (!Array.isArray(value) || value.length > most) throw invalid(field);
  return value;
}
function schema(value: Record<string, unknown>): void {
  if (value.schema_version !== 1) throw invalid('schema_version');
}

/** A failure the desktop named, or `internal` for anything that is not one. */
export function parseF8Failure(value: unknown): F8Failure {
  try {
    const v = record(value, ['code', 'retry', 'retry_in_ms'], 'failure');
    return {
      code: member(v.code, F8_FAILURES, 'code'),
      retry: flag(v.retry, 'retry'),
      retry_in_ms: nullable(v.retry_in_ms, (x) => count(x, 'retry_in_ms')),
    };
  } catch {
    return { code: 'internal', retry: false, retry_in_ms: null };
  }
}

function parseAuthority(value: unknown): AuthorityInfo {
  const v = record(
    value,
    [
      'instance_id',
      'service_version',
      'protocol_version',
      'storage_schema_version',
      'stream_id',
      'last_sequence',
      'prepared_count',
      'closed_count',
    ],
    'authority',
  );
  if (v.protocol_version !== 2) throw invalid('protocol_version');
  if (v.storage_schema_version !== 8) throw invalid('storage_schema_version');
  if (
    typeof v.service_version !== 'string' ||
    v.service_version.length === 0 ||
    v.service_version.length > 64
  )
    throw invalid('service_version');
  return {
    instance_id: uuid(v.instance_id, 'instance_id'),
    service_version: v.service_version,
    protocol_version: 2,
    storage_schema_version: 8,
    stream_id: uuid(v.stream_id, 'stream_id'),
    last_sequence: nullable(v.last_sequence, (x) =>
      sequence(x, 'last_sequence'),
    ),
    prepared_count: count(v.prepared_count, 'prepared_count'),
    closed_count: count(v.closed_count, 'closed_count'),
  };
}

export function parseF8State(value: unknown): F8State {
  const v = record(
    value,
    ['schema_version', 'active', 'link', 'authority', 'epoch'],
    'state',
  );
  schema(v);
  const link = record(v.link, ['state', 'fault', 'retry_in_ms'], 'link');
  const active = flag(v.active, 'active');
  const state = member(link.state, LINK_STATES, 'link');
  // An interface that is not active has no link and no authority: nothing is shown of one.
  if (!active && (state !== 'stopped' || v.authority !== null))
    throw invalid('state');
  const authority = nullable(v.authority, parseAuthority);
  if (authority !== null && state !== 'connected') throw invalid('authority');
  return {
    schema_version: 1,
    active,
    link: {
      state,
      fault: nullable(link.fault, (x) => member(x, LINK_FAULTS, 'fault')),
      retry_in_ms: nullable(link.retry_in_ms, (x) => count(x, 'retry_in_ms')),
    },
    authority,
    epoch: count(v.epoch, 'epoch'),
  };
}

function parseFence(value: unknown): Fence {
  const v = record(value, ['session', 'generation', 'instance'], 'fence');
  const generation = count(v.generation, 'generation');
  const instance = instanceId(v.instance);
  return { session: sessionId(v.session), generation, instance };
}

function parseRuntime(value: unknown, session: ManagedSessionId): RuntimeInfo {
  const v = record(
    value,
    [
      'desired',
      'actual',
      'revision',
      'generation',
      'instance',
      'version',
      'profile',
      'workspace',
      'model_count',
      'diagnostic',
      'budget_failure',
      'fence',
    ],
    'runtime',
  );
  const generation = count(v.generation, 'generation');
  const instance = instanceId(v.instance);
  const fence = parseFence(v.fence);
  // The fence is the observation this very row carries: no other.
  if (
    !same(fence.session, session) ||
    fence.generation !== generation ||
    (fence.instance ?? '') !== (instance ?? '')
  )
    throw invalid('fence');
  const actual = member(v.actual, ACTUAL_STATES, 'actual');
  const profile = nullable(v.profile, (x) =>
    member(x, RUNTIME_PROFILES, 'profile'),
  );
  const workspace = nullable(v.workspace, folder);
  // Only a local-tools generation is isolated for a workspace, and it always is.
  if ((workspace !== null) !== (profile === 'codex_local_tools_v1'))
    throw invalid('workspace');
  const version = nullable(v.version, (x) => {
    if (typeof x !== 'string' || x.length > 128 || !VERSION.test(x))
      throw invalid('version');
    return x;
  });
  const modelCount = count(v.model_count, 'model_count');
  if (modelCount > 50 || (actual !== 'ready' && modelCount !== 0))
    throw invalid('model_count');
  // A ready runtime is attested: it has an identity, a version and a generation.
  if (
    actual === 'ready' &&
    (instance === null || version === null || generation < 1)
  )
    throw invalid('ready');
  return {
    desired: member(v.desired, DESIRED_STATES, 'desired'),
    actual,
    revision: count(v.revision, 'revision'),
    generation,
    instance,
    version,
    profile,
    workspace,
    model_count: modelCount,
    diagnostic: nullable(v.diagnostic, (x) =>
      member(x, RUNTIME_DIAGNOSTICS, 'diagnostic'),
    ),
    budget_failure: nullable(v.budget_failure, (x) => {
      if (typeof x !== 'string' || !BUDGET_KEY.test(x)) throw invalid('budget');
      return x;
    }),
    fence,
  };
}

function parseSessionRow(value: unknown): SessionRow {
  const v = record(
    value,
    [
      'id',
      'state',
      'revision',
      'created_at',
      'updated_at',
      'position',
      'runtime',
    ],
    'session',
  );
  const id = sessionId(v.id);
  const state = member(v.state, SESSION_STATES, 'state');
  // A managed session is at revision one while prepared and two once closed.
  if (v.revision !== (state === 'prepared' ? 1 : 2)) throw invalid('revision');
  const runtime = parseRuntime(v.runtime, id);
  return {
    id,
    state,
    revision: v.revision as number,
    created_at: timestamp(v.created_at, 'created_at'),
    updated_at: timestamp(v.updated_at, 'updated_at'),
    position: sequence(v.position, 'position'),
    runtime,
  };
}

export function parseSessionsView(value: unknown): SessionsView {
  const v = record(
    value,
    ['schema_version', 'authority', 'head', 'sessions', 'older', 'capacity'],
    'sessions',
  );
  schema(v);
  const sessions = array(v.sessions, SESSION_PAGE_LIMIT, 'sessions').map(
    parseSessionRow,
  );
  // Newest first, each once.
  for (let i = 1; i < sessions.length; i++)
    if (BigInt(sessions[i].position) >= BigInt(sessions[i - 1].position))
      throw invalid('order');
  if (new Set(sessions.map((s) => s.id.toLowerCase())).size !== sessions.length)
    throw invalid('sessions');
  const older = nullable(v.older, (x) => sequence(x, 'older'));
  // The cursor of the next page is where this one ends.
  if (older !== null && older !== sessions.at(-1)?.position)
    throw invalid('older');
  return {
    schema_version: 1,
    authority: uuid(v.authority, 'authority'),
    head: nullable(v.head, (x) => sequence(x, 'head')),
    sessions,
    older,
    capacity: count(v.capacity, 'capacity'),
  };
}

function parseBinding(value: unknown): BindingInfo {
  const v = record(
    value,
    [
      'state',
      'workspace',
      'mode',
      'operation',
      'observed_model',
      'attached_instance',
    ],
    'binding',
  );
  const state = member(v.state, BINDING_STATES, 'binding');
  const workspace = nullable(v.workspace, folder);
  const attached = state === 'bound_attached';
  const bound = attached || state === 'bound_detached';
  const mode = nullable(v.mode, (x) => member(x, ATTACHMENT_PROFILES, 'mode'));
  const observed = parseObservedModel(v.observed_model);
  const instance = instanceId(v.attached_instance);
  // The workspace is the binding's; a mode, a model and an instance are the attachment's.
  if (
    (workspace !== null) !== bound ||
    (!attached && (mode !== null || observed !== null)) ||
    (instance !== null) !== attached
  )
    throw invalid('binding');
  const operation = nullable(v.operation, (x) => uuid(x, 'operation'));
  // The operation an indeterminate create left, or the one still being created.
  if (operation !== null && state !== 'indeterminate' && state !== 'creating')
    throw invalid('operation');
  return {
    state,
    workspace,
    mode,
    operation,
    observed_model: observed,
    attached_instance: instance,
  };
}

function parseLease(value: unknown): LeaseInfo {
  const v = record(
    value,
    [
      'id',
      'state',
      'scopes',
      'revision',
      'issued_at',
      'expires_at',
      'revocation',
    ],
    'lease',
  );
  const scopes = array(v.scopes, LEASE_SCOPES.length, 'scopes').map((s) =>
    member(s, LEASE_SCOPES, 'scopes'),
  );
  if (scopes.length === 0 || new Set(scopes).size !== scopes.length)
    throw invalid('scopes');
  const state = member(v.state, LEASE_STATES, 'state');
  const revocation = nullable(v.revocation, (x) =>
    member(x, REVOCATIONS, 'revocation'),
  );
  // A lease is revoked exactly when it says why.
  if ((revocation !== null) !== (state === 'revoked')) throw invalid('lease');
  const issued = timestamp(v.issued_at, 'issued_at');
  const expires = timestamp(v.expires_at, 'expires_at');
  if (Date.parse(expires) <= Date.parse(issued)) throw invalid('lease');
  return {
    id: uuid(v.id, 'id'),
    state,
    scopes,
    revision: count(v.revision, 'revision', 1),
    issued_at: issued,
    expires_at: expires,
    revocation,
  };
}

function parseGate(value: unknown): GateInfo {
  const v = record(value, ['action', 'reason'], 'gate');
  return {
    action: member(v.action, GATE_ACTIONS, 'action'),
    reason: member(v.reason, GATE_REASONS, 'reason'),
  };
}

function parseTurnInfo(value: unknown): TurnInfo {
  const v = record(value, ['turn', 'generation', 'live'], 'turn');
  const generation = nullable(v.generation, (x) => count(x, 'generation', 1));
  const live = flag(v.live, 'live');
  // A launch is live only for a turn that belongs to one.
  if (live && generation === null) throw invalid('turn');
  return { turn: parseTurnSummary(v.turn), generation, live };
}

export function parseSessionDetail(value: unknown): SessionDetail {
  const v = record(
    value,
    [
      'schema_version',
      'authority',
      'session',
      'binding',
      'lease',
      'gates',
      'turns',
      'turns_partial',
      'approval',
    ],
    'detail',
  );
  schema(v);
  const session = parseSessionRow(v.session);
  const turns = array(v.turns, TURN_PAGE_LIMIT, 'turns').map(parseTurnInfo);
  if (turns.some((t) => !same(t.turn.session, session.id)))
    throw invalid('turns');
  for (let i = 1; i < turns.length; i++)
    if (
      Date.parse(turns[i].turn.started_at) >
      Date.parse(turns[i - 1].turn.started_at)
    )
      throw invalid('order');
  const approval = nullable(v.approval, parseApprovalDetail);
  if (approval !== null && !same(approval.approval.session, session.id))
    throw invalid('approval');
  const gates = array(v.gates, GATE_ACTIONS.length, 'gates').map(parseGate);
  if (new Set(gates.map((g) => g.action)).size !== gates.length)
    throw invalid('gates');
  return {
    schema_version: 1,
    authority: uuid(v.authority, 'authority'),
    session,
    binding: parseBinding(v.binding),
    lease: nullable(v.lease, parseLease),
    gates,
    turns,
    turns_partial: flag(v.turns_partial, 'turns_partial'),
    approval,
  };
}

export function parseTurnReading(value: unknown): TurnReading {
  const v = record(
    value,
    [
      'schema_version',
      'authority',
      'turn',
      'events',
      'next',
      'more',
      'gap',
      'approval',
    ],
    'reading',
  );
  schema(v);
  const turn = parseTurnInfo(v.turn);
  const events = array(v.events, TURN_EVENTS_LIMIT, 'events').map(
    parseTurnEvent,
  );
  for (let i = 1; i < events.length; i++)
    if (BigInt(events[i].sequence) <= BigInt(events[i - 1].sequence))
      throw invalid('order');
  const next = nullable(v.next, (x) => sequence(x, 'next'));
  const last = events.at(-1);
  if (last && next !== last.sequence) throw invalid('next');
  const approval = nullable(v.approval, parseApprovalNotice);
  // A turn that ended waits on nothing.
  if (approval && turnEnded(turn.turn.state)) throw invalid('approval');
  return {
    schema_version: 1,
    authority: uuid(v.authority, 'authority'),
    turn,
    events,
    next,
    more: flag(v.more, 'more'),
    gap: flag(v.gap, 'gap'),
    approval,
  };
}

function receipt<T extends Record<string, unknown>>(
  value: unknown,
  keys: readonly string[],
): T & { authority: Uuid } {
  const v = record(value, ['schema_version', 'authority', ...keys], 'receipt');
  schema(v);
  return { ...v, authority: uuid(v.authority, 'authority') } as T & {
    authority: Uuid;
  };
}
export function parseSessionReceipt(value: unknown): SessionReceipt {
  const v = receipt(value, ['id']);
  return { schema_version: 1, authority: v.authority, id: sessionId(v.id) };
}
export function parseTurnReceipt(value: unknown): TurnReceipt {
  const v = receipt(value, ['id']);
  return { schema_version: 1, authority: v.authority, id: uuid(v.id, 'id') };
}
export function parseLeaseReceipt(value: unknown): LeaseReceipt {
  const v = receipt(value, ['lease']);
  return {
    schema_version: 1,
    authority: v.authority,
    lease: parseLease(v.lease),
  };
}
export function parseRuntimeReceipt(value: unknown): RuntimeReceipt {
  const v = receipt(value, ['session', 'desired', 'revision', 'generation']);
  return {
    schema_version: 1,
    authority: v.authority,
    session: sessionId(v.session),
    desired: member(v.desired, DESIRED_STATES, 'desired'),
    revision: count(v.revision, 'revision', 1),
    generation: count(v.generation, 'generation', 1),
  };
}
export function parseBindingReceipt(value: unknown): BindingReceipt {
  const v = receipt(value, ['session', 'workspace']);
  return {
    schema_version: 1,
    authority: v.authority,
    session: sessionId(v.session),
    workspace: folder(v.workspace),
  };
}
export function parseAbandonReceipt(value: unknown): AbandonReceipt {
  const v = receipt(value, ['session']);
  return {
    schema_version: 1,
    authority: v.authority,
    session: sessionId(v.session),
  };
}
export function parseDecisionReceipt(value: unknown): DecisionReceipt {
  const v = receipt(value, ['id', 'state']);
  const state = member(v.state, APPROVAL_STATES, 'state');
  // A decision the authority committed is never a pending approval.
  if (state === 'pending') throw invalid('receipt');
  return {
    schema_version: 1,
    authority: v.authority,
    id: uuid(v.id, 'id'),
    state,
  };
}

export function parseModelsInfo(value: unknown): ModelsInfo {
  const v = record(
    value,
    [
      'schema_version',
      'authority',
      'fence',
      'version',
      'complete',
      'entries',
      'observed',
    ],
    'models',
  );
  schema(v);
  if (typeof v.version !== 'string' || !VERSION.test(v.version))
    throw invalid('version');
  const entries = array(v.entries, 50, 'entries').map(parseCatalogueEntry);
  if (new Set(entries.map((e) => e.model_id)).size !== entries.length)
    throw invalid('entries');
  if (v.complete !== null && typeof v.complete !== 'boolean')
    throw invalid('complete');
  const fence = parseFence(v.fence);
  if (fence.instance === null || fence.generation < 1) throw invalid('fence');
  return {
    schema_version: 1,
    authority: uuid(v.authority, 'authority'),
    fence,
    version: v.version,
    complete: v.complete,
    entries,
    observed: parseObservedModel(v.observed),
  };
}

export function parseAuditInfo(value: unknown): AuditInfo {
  const v = record(value, ['schema_version', 'authority', 'page'], 'audit');
  schema(v);
  return {
    schema_version: 1,
    authority: uuid(v.authority, 'authority'),
    page: parseEventsPage(v.page),
  };
}

export function parseWatchInfo(value: unknown): WatchInfo {
  const v = record(
    value,
    ['schema_version', 'authority', 'stream', 'head', 'changed'],
    'watch',
  );
  schema(v);
  return {
    schema_version: 1,
    authority: uuid(v.authority, 'authority'),
    stream: uuid(v.stream, 'stream'),
    head: nullable(v.head, (x) => sequence(x, 'head')),
    changed: flag(v.changed, 'changed'),
  };
}
