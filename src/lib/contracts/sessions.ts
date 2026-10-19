/**
 * SessionPage v3: the desktop's projection of managed sessions, and what its session
 * intents answer. It is not the control contract: it names no control method, and the
 * only identity it carries is the session's own.
 */
import { parseManagedSessionId, type ManagedSessionId } from './identities';

export type SessionState = 'prepared' | 'closed';
export type SessionBinding =
  | 'unbound'
  | 'creating'
  | 'indeterminate'
  | 'bound_detached'
  | 'bound_attached';

/**
 * What a session can do, as a product choice: text only, or text with commands and file
 * changes that each wait for an approval. One of exactly these two.
 */
export const AGENT_MODES = ['inference', 'local_tools'] as const;
export type AgentMode = (typeof AGENT_MODES)[number];
/** The mode a session is attached in. `unknown` while it is not attached. */
export type SessionMode = AgentMode | 'unknown';
/** The one managed runtime every session shares. */
export interface RuntimeFacts {
  /** The mode of the generation that is ready now. Null when none is. */
  mode: AgentMode | null;
  /** The one workspace a local-tools generation serves. */
  workspace: string | null;
}

export interface SessionSummary {
  id: ManagedSessionId;
  adapter: string;
  state: SessionState;
  revision: number;
  created_at: string;
  updated_at: string;
  /** Canonical decimal position in creation order. It is the paging cursor. */
  position: string;
  binding: SessionBinding;
  workspace: string | null;
  mode: SessionMode;
  /** Presentation hints from the desktop. The authority decides. */
  resumable: boolean;
  closable: boolean;
  /** Prepared, with no binding and no attempt at one: a workspace can be bound. */
  activatable: boolean;
  /** Prepared, with an attempt whose outcome the authority could not confirm. */
  recoverable: boolean;
  /**
   * The model the managed runtime itself reported for this session, only while it is attached.
   * Null means "not reported": it is never the default, a catalogue entry or a configured value.
   */
  observed_model: ObservedModel | null;
}
export interface SessionPage {
  schema_version: 4;
  /** The durable head the page was read at; null before the first committed event. */
  head: string | null;
  runtime: RuntimeFacts;
  /** Newest first. */
  sessions: SessionSummary[];
  /** Cursor of the next older page, when older sessions exist. */
  older: string | null;
  truncated: boolean;
}
export interface SessionReceipt {
  id: ManagedSessionId;
}

/**
 * How far one step of an activation got, as the authority answered it. The same
 * request is sent again until the stage is `ready`.
 */
export const ACTIVATION_STAGES = [
  'runtime_stopping',
  'runtime_starting',
  'creating',
  'bound',
  'ready',
] as const;
export type ActivationStage = (typeof ACTIVATION_STAGES)[number];
export interface ActivationProgress {
  stage: ActivationStage;
}
/** The one folder the user selected in the native dialog. */
export interface WorkspaceChoice {
  path: string;
}

export const SESSION_FAILURES = [
  'unavailable',
  'incompatible',
  'invalid',
  'not_found',
  'changed',
  'not_allowed',
  'not_bound',
  'runtime_unavailable',
  'start_refused',
  'turn_active',
  'mode_fixed',
  'workspace_invalid',
  'already_bound',
  'permission_denied',
  'busy',
  'rejected',
  'uncertain',
  'conflict',
  'internal',
] as const;
export type SessionFailureCode = (typeof SESSION_FAILURES)[number];
export interface SessionFailure {
  code: SessionFailureCode;
  /** The outcome is unknown or nothing was refused: the same intent may be sent again. */
  retry: boolean;
}

/** Sessions one page may hold. The desktop sends at most this many. */
export const SESSION_PAGE_LIMIT = 25;

const STATES: readonly SessionState[] = ['prepared', 'closed'];
const BINDINGS: readonly SessionBinding[] = [
  'unbound',
  'creating',
  'indeterminate',
  'bound_detached',
  'bound_attached',
];
const SEQUENCE = /^[1-9][0-9]{0,19}$/;
const TIMESTAMP =
  /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d{1,9})?([Zz]|\+00:00)$/;

function invalid(field: string): Error {
  return new Error(`session_page: invalid_${field}`);
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
/** An absolute path of bounded length with no control character, as the authority keeps it. */
function workspace(value: unknown): string | null {
  if (value === null) return null;
  return path(value);
}
function path(value: unknown): string {
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

const MODES: readonly SessionMode[] = [...AGENT_MODES, 'unknown'];
function parseRuntime(value: unknown): RuntimeFacts {
  const v = record(value, ['mode', 'workspace'], 'runtime');
  const mode = v.mode === null ? null : member(v.mode, AGENT_MODES, 'mode');
  const folder = workspace(v.workspace);
  // Only a local-tools generation is isolated for a workspace, and it always is.
  if ((folder !== null) !== (mode === 'local_tools')) throw invalid('runtime');
  return { mode, workspace: folder };
}

export interface ObservedModel {
  model_id: string;
  provider_id: string | null;
  reasoning_effort: string | null;
  source: 'thread_start' | 'thread_resume';
}
const WORD = /^[\x21-\x7e]+$/;
function parseObservedModel(value: unknown): ObservedModel | null {
  if (value === null) return null;
  const v = record(
    value,
    ['model_id', 'provider_id', 'reasoning_effort', 'source'],
    'observed_model',
  );
  const word = (x: unknown, most: number): string | null => {
    if (x === null) return null;
    if (
      typeof x !== 'string' ||
      x.length === 0 ||
      x.length > most ||
      !WORD.test(x)
    )
      throw invalid('observed_model');
    return x;
  };
  const model = word(v.model_id, 256);
  if (model === null) throw invalid('observed_model');
  if (v.source !== 'thread_start' && v.source !== 'thread_resume')
    throw invalid('observed_model');
  return {
    model_id: model,
    provider_id: word(v.provider_id, 64),
    reasoning_effort: word(v.reasoning_effort, 64),
    source: v.source,
  };
}

function parseSummary(value: unknown): SessionSummary {
  const v = record(
    value,
    [
      'id',
      'adapter',
      'state',
      'revision',
      'created_at',
      'updated_at',
      'position',
      'binding',
      'workspace',
      'mode',
      'resumable',
      'closable',
      'activatable',
      'recoverable',
      'observed_model',
    ],
    'session',
  );
  let id: ManagedSessionId;
  try {
    id = parseManagedSessionId(v.id);
  } catch {
    throw invalid('id');
  }
  if (
    typeof v.adapter !== 'string' ||
    !/^[a-z][a-z0-9_-]{0,63}$/.test(v.adapter)
  )
    throw invalid('adapter');
  const state = member(v.state, STATES, 'state');
  // A managed session is at revision one while prepared and two once closed.
  if (v.revision !== (state === 'prepared' ? 1 : 2)) throw invalid('revision');
  const binding = member(v.binding, BINDINGS, 'binding');
  const folder = workspace(v.workspace);
  const bound = binding === 'bound_detached' || binding === 'bound_attached';
  if ((folder !== null) !== bound) throw invalid('workspace');
  const mode = member(v.mode, MODES, 'mode');
  // A mode is what an attachment reports: a session that is not attached has none.
  if (mode !== 'unknown' && binding !== 'bound_attached') throw invalid('mode');
  const resumable = flag(v.resumable, 'resumable');
  const closable = flag(v.closable, 'closable');
  const activatable = flag(v.activatable, 'activatable');
  const recoverable = flag(v.recoverable, 'recoverable');
  // No hint may offer what the shown state already rules out.
  if (
    (state === 'closed' &&
      (resumable || closable || activatable || recoverable)) ||
    (resumable && !bound) ||
    (activatable && binding !== 'unbound') ||
    (recoverable && binding !== 'indeterminate') ||
    (state === 'closed' && binding === 'bound_attached')
  )
    throw invalid('hints');
  const observed = parseObservedModel(v.observed_model);
  // A report belongs to an attachment: a session that is not attached has none.
  if (observed !== null && binding !== 'bound_attached')
    throw invalid('observed_model');
  return {
    id,
    adapter: v.adapter,
    state,
    revision: v.revision as number,
    created_at: timestamp(v.created_at, 'created_at'),
    updated_at: timestamp(v.updated_at, 'updated_at'),
    position: sequence(v.position, 'position'),
    binding,
    workspace: folder,
    mode,
    resumable,
    closable,
    activatable,
    recoverable,
    observed_model: observed,
  };
}

/** Validates the whole page. Anything unexpected is an error, never a guess. */
export function parseSessionPage(value: unknown): SessionPage {
  const v = record(
    value,
    ['schema_version', 'head', 'runtime', 'sessions', 'older', 'truncated'],
    'page',
  );
  if (v.schema_version !== 4) throw invalid('schema_version');
  if (!Array.isArray(v.sessions) || v.sessions.length > SESSION_PAGE_LIMIT)
    throw invalid('sessions');
  const sessions = v.sessions.map(parseSummary);
  const head = v.head === null ? null : sequence(v.head, 'head');
  // Newest first. The head is read before the sessions, so a session committed in
  // between may sit past it: the head is a lower bound, not a limit.
  for (let i = 1; i < sessions.length; i++)
    if (BigInt(sessions[i].position) >= BigInt(sessions[i - 1].position))
      throw invalid('order');
  if (new Set(sessions.map((s) => s.id.toLowerCase())).size !== sessions.length)
    throw invalid('sessions');
  const older = v.older === null ? null : sequence(v.older, 'older');
  // The cursor of the next page is where this one ends.
  if (older !== null && older !== sessions.at(-1)?.position)
    throw invalid('older');
  return {
    schema_version: 4,
    head,
    runtime: parseRuntime(v.runtime),
    sessions,
    older,
    truncated: flag(v.truncated, 'truncated'),
  };
}

export function parseSessionReceipt(value: unknown): SessionReceipt {
  const v = record(value, ['id'], 'receipt');
  try {
    return { id: parseManagedSessionId(v.id) };
  } catch {
    throw invalid('receipt');
  }
}

export function parseActivationProgress(value: unknown): ActivationProgress {
  const v = record(value, ['stage'], 'progress');
  return { stage: member(v.stage, ACTIVATION_STAGES, 'stage') };
}

/** The folder the dialog returned, or null when it was dismissed. */
export function parseWorkspaceChoice(value: unknown): WorkspaceChoice | null {
  if (value === null) return null;
  const v = record(value, ['path'], 'choice');
  try {
    return { path: path(v.path) };
  } catch {
    throw invalid('choice');
  }
}

/** A failure the desktop named, or `internal` for anything that is not one. */
export function parseSessionFailure(value: unknown): SessionFailure {
  try {
    const v = record(value, ['code', 'retry'], 'failure');
    return {
      code: member(v.code, SESSION_FAILURES, 'code'),
      retry: flag(v.retry, 'retry'),
    };
  } catch {
    return { code: 'internal', retry: false };
  }
}
