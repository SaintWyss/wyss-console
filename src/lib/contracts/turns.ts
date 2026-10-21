/**
 * TurnView v2, TurnList v3 and TurnHistory v1: the desktop's projection of managed turns, and what its
 * turn intents answer. It is not the control contract. It names no control method and
 * carries no provider identity, no lease, no runtime instance, no reasoning text and no
 * detail of a mediated action: the only provider text in it is the assistant's answer.
 */
import {
  parseManagedApprovalId,
  parseManagedSessionId,
  parseManagedTurnId,
  type ManagedApprovalId,
  type ManagedSessionId,
  type ManagedTurnId,
} from './identities';
import { parseUuid, type Uuid } from './uuid';
import {
  AGENT_MODES,
  type SessionBinding,
  type SessionMode,
  type SessionState,
} from './sessions';

/** The authority's own lifecycle names. The last four are final. */
export const TURN_STATES = [
  'starting',
  'running',
  'completed',
  'interrupted',
  'failed',
  'indeterminate',
] as const;
export type TurnState = (typeof TURN_STATES)[number];
const FINAL: readonly TurnState[] = [
  'completed',
  'interrupted',
  'failed',
  'indeterminate',
];
export function turnEnded(state: TurnState): boolean {
  return FINAL.includes(state);
}

/** Why a turn ended, as the authority recorded it. */
export const TURN_REASONS = [
  'completed',
  'user_cancel',
  'lease_expired',
  'lease_revoked',
  'session_closed',
  'runtime_stopped',
  'authority_shutdown',
  'authority_restarted',
  'runtime_failed',
  'provider_failed',
  'dispatch_unknown',
  'inference_isolation_violation',
  'interrupt_timeout',
  'approval_expired',
  'action_rejected',
] as const;
export type TurnReason = (typeof TURN_REASONS)[number];

/**
 * What one stream event is. Only `text` carries what the provider wrote; a mediated
 * action is named by what happened to it, never by its content.
 */
export const TURN_EVENT_KINDS = [
  'started',
  'text',
  'message_end',
  'thinking',
  'completed',
  'interrupted',
  'failed',
  'gap',
  'command_proposed',
  'command_started',
  'command_output',
  'command_completed',
  'command_failed',
  'file_change_proposed',
  'file_change_started',
  'file_change_completed',
  'file_change_failed',
  'approval_required',
  'approval_resolved',
] as const;
export type TurnEventKind = (typeof TURN_EVENT_KINDS)[number];

export const APPROVAL_KINDS = ['command_execution', 'file_change'] as const;
export type ApprovalKind = (typeof APPROVAL_KINDS)[number];
/** The states an approval ends in, as `approval_resolved` reports them. */
export const APPROVAL_OUTCOMES = [
  'accepted',
  'declined',
  'cancelled',
  'expired',
  'invalidated',
  'indeterminate',
] as const;
export type ApprovalOutcome = (typeof APPROVAL_OUTCOMES)[number];

export interface TurnSummary {
  id: ManagedTurnId;
  session: ManagedSessionId;
  state: TurnState;
  revision: number;
  started_at: string;
  updated_at: string;
  ended_at: string | null;
  reason: TurnReason | null;
  /** The authority still holds output of this turn to read. */
  output: boolean;
  /** Presentation hint: the turn has not ended. The authority decides a cancel. */
  cancellable: boolean;
}
export interface TurnEvent {
  /** Canonical decimal position in the turn's stream. Only ever handed back as a cursor. */
  sequence: string;
  kind: TurnEventKind;
  text: string | null;
}
/**
 * The approval a turn waits on: which one, what kind of action and until when. The
 * identity is a handle: what the approval is about is read with it.
 */
export interface ApprovalNotice {
  id: ManagedApprovalId;
  kind: ApprovalKind;
  expires_at: string;
}
export interface TurnView {
  schema_version: 2;
  turn: TurnSummary;
  events: TurnEvent[];
  /** The cursor to read from next. Unchanged when nothing new was read. */
  next: string | null;
  more: boolean;
  /** Output before what is returned was not kept by the authority. */
  gap: boolean;
  approval: ApprovalNotice | null;
}
export interface TurnReceipt {
  id: ManagedTurnId;
}
export interface SessionFacts {
  id: ManagedSessionId;
  state: SessionState;
  revision: number;
  binding: SessionBinding;
  workspace: string | null;
  /** The mode the session is attached in. `unknown` while it is not attached. */
  mode: SessionMode;
}
/**
 * Where older turns are looked for: a position of one durable stream. The window is handed
 * it and hands it back as it was; it decides nothing from it. Turns admitted at or after
 * `before` have been looked through for the session.
 */
export interface HistoryCursor {
  stream_id: Uuid;
  before: string;
}
export interface TurnList {
  schema_version: 3;
  head: string | null;
  session: SessionFacts;
  /** Newest first. */
  turns: TurnSummary[];
  /** Presentation hint. Whether a turn may start is the authority's to say. */
  runnable: boolean;
  /** Older history exists that the desktop did not read. */
  partial: boolean;
  /**
   * Where the turns before the newest ones begin. Null only when the desktop has seen
   * the whole history of the session and nothing older exists.
   */
  older: HistoryCursor | null;
}
/**
 * One page of the turns before those already shown, newest first. A turn is here because
 * the authority's durable stream says it was admitted for this session, and it is what
 * the authority holds of it now: its output is usually gone, and that is said, not filled in.
 */
export interface TurnHistory {
  schema_version: 1;
  session: ManagedSessionId;
  turns: TurnSummary[];
  /** Where the next older page begins. Null when the start of history was reached. */
  older: HistoryCursor | null;
}

/** Turns one list may hold, and events one reading may hold. The desktop sends no more. */
export const TURN_LIST_LIMIT = 20;
export const TURN_EVENTS_LIMIT = 50;
/** What the user may write for one turn, in UTF-8 bytes: the protocol's own bound. */
export const PROMPT_MAX_BYTES = 8192;
/** One piece of the answer is at most this long: the authority cuts longer ones. */
const TEXT_MAX_BYTES = 4096;

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
  return new Error(`turn_view: invalid_${field}`);
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
function revision(value: unknown): number {
  if (!Number.isSafeInteger(value) || (value as number) < 1)
    throw invalid('revision');
  return value as number;
}
function session(value: unknown): ManagedSessionId {
  try {
    return parseManagedSessionId(value);
  } catch {
    throw invalid('session');
  }
}
function turnId(value: unknown): ManagedTurnId {
  try {
    return parseManagedTurnId(value);
  } catch {
    throw invalid('id');
  }
}

export function parseTurnSummary(value: unknown): TurnSummary {
  const v = record(
    value,
    [
      'id',
      'session',
      'state',
      'revision',
      'started_at',
      'updated_at',
      'ended_at',
      'reason',
      'output',
      'cancellable',
    ],
    'turn',
  );
  const state = member(v.state, TURN_STATES, 'state');
  const ended = turnEnded(state);
  // A turn that ended says when and why; one that has not, says neither.
  if ((v.ended_at !== null) !== ended || (v.reason !== null) !== ended)
    throw invalid('end');
  const cancellable = flag(v.cancellable, 'cancellable');
  // No hint may offer what the shown state already rules out.
  if (cancellable === ended) throw invalid('hints');
  return {
    id: turnId(v.id),
    session: session(v.session),
    state,
    revision: revision(v.revision),
    started_at: timestamp(v.started_at, 'started_at'),
    updated_at: timestamp(v.updated_at, 'updated_at'),
    ended_at: v.ended_at === null ? null : timestamp(v.ended_at, 'ended_at'),
    reason: v.reason === null ? null : member(v.reason, TURN_REASONS, 'reason'),
    output: flag(v.output, 'output'),
    cancellable,
  };
}

export function parseTurnEvent(value: unknown): TurnEvent {
  const v = record(value, ['sequence', 'kind', 'text'], 'event');
  const kind = member(v.kind, TURN_EVENT_KINDS, 'kind');
  let text: string | null = null;
  if (kind === 'text') {
    // A piece of the answer is text and nothing else: it is never parsed further.
    if (
      typeof v.text !== 'string' ||
      new TextEncoder().encode(v.text).length > TEXT_MAX_BYTES
    )
      throw invalid('text');
    text = v.text;
  } else if (kind === 'approval_resolved') {
    text = v.text === null ? null : member(v.text, APPROVAL_OUTCOMES, 'text');
  } else if (v.text !== null) {
    // Nothing else carries text. One that does is not this projection.
    throw invalid('text');
  }
  return { sequence: sequence(v.sequence, 'sequence'), kind, text };
}

export function parseApprovalNotice(value: unknown): ApprovalNotice {
  const v = record(value, ['id', 'kind', 'expires_at'], 'approval');
  let id: ManagedApprovalId;
  try {
    id = parseManagedApprovalId(v.id);
  } catch {
    throw invalid('approval');
  }
  return {
    id,
    kind: member(v.kind, APPROVAL_KINDS, 'approval'),
    expires_at: timestamp(v.expires_at, 'expires_at'),
  };
}

/** Validates the whole reading. Anything unexpected is an error, never a guess. */
export function parseTurnView(value: unknown): TurnView {
  const v = record(
    value,
    ['schema_version', 'turn', 'events', 'next', 'more', 'gap', 'approval'],
    'view',
  );
  if (v.schema_version !== 2) throw invalid('schema_version');
  const turn = parseTurnSummary(v.turn);
  if (!Array.isArray(v.events) || v.events.length > TURN_EVENTS_LIMIT)
    throw invalid('events');
  const events = v.events.map(parseTurnEvent);
  // In stream order, each position once.
  for (let i = 1; i < events.length; i++)
    if (BigInt(events[i].sequence) <= BigInt(events[i - 1].sequence))
      throw invalid('order');
  const next = v.next === null ? null : sequence(v.next, 'next');
  // The cursor of the next reading is where this one ends.
  const last = events.at(-1);
  if (last && next !== last.sequence) throw invalid('next');
  const approval = v.approval === null ? null : parseApprovalNotice(v.approval);
  // A turn that ended waits on nothing.
  if (approval && turnEnded(turn.state)) throw invalid('approval');
  return {
    schema_version: 2,
    turn,
    events,
    next,
    more: flag(v.more, 'more'),
    gap: flag(v.gap, 'gap'),
    approval,
  };
}

function parseFacts(value: unknown): SessionFacts {
  const v = record(
    value,
    ['id', 'state', 'revision', 'binding', 'workspace', 'mode'],
    'session',
  );
  const state = member(v.state, STATES, 'session_state');
  if (v.revision !== (state === 'prepared' ? 1 : 2)) throw invalid('revision');
  const binding = member(v.binding, BINDINGS, 'binding');
  const bound = binding === 'bound_detached' || binding === 'bound_attached';
  if (
    v.workspace !== null &&
    (typeof v.workspace !== 'string' ||
      !v.workspace.startsWith('/') ||
      new TextEncoder().encode(v.workspace).length > 256 ||
      // eslint-disable-next-line no-control-regex
      /[\u0000-\u001f\u007f-\u009f]/.test(v.workspace))
  )
    throw invalid('workspace');
  if ((v.workspace !== null) !== bound) throw invalid('workspace');
  const mode = member(v.mode, [...AGENT_MODES, 'unknown'] as const, 'mode');
  // A mode is what an attachment reports: a session that is not attached has none.
  if (mode !== 'unknown' && binding !== 'bound_attached') throw invalid('mode');
  return {
    id: session(v.id),
    state,
    revision: v.revision as number,
    binding,
    workspace: v.workspace as string | null,
    mode,
  };
}

export function parseHistoryCursor(value: unknown): HistoryCursor {
  const v = record(value, ['stream_id', 'before'], 'cursor');
  let stream: Uuid;
  try {
    stream = parseUuid(v.stream_id);
  } catch {
    throw invalid('cursor');
  }
  // Position one is the first: nothing is before it, so no cursor names it.
  if (sequence(v.before, 'cursor') === '1') throw invalid('cursor');
  return { stream_id: stream, before: v.before as string };
}

export function parseTurnList(value: unknown): TurnList {
  const v = record(
    value,
    [
      'schema_version',
      'head',
      'session',
      'turns',
      'runnable',
      'partial',
      'older',
    ],
    'list',
  );
  if (v.schema_version !== 3) throw invalid('schema_version');
  const facts = parseFacts(v.session);
  if (!Array.isArray(v.turns) || v.turns.length > TURN_LIST_LIMIT)
    throw invalid('turns');
  const turns = v.turns.map(parseTurnSummary);
  // Every turn is this session's, and each is listed once.
  if (turns.some((t) => t.session.toLowerCase() !== facts.id.toLowerCase()))
    throw invalid('turns');
  if (new Set(turns.map((t) => t.id.toLowerCase())).size !== turns.length)
    throw invalid('turns');
  const runnable = flag(v.runnable, 'runnable');
  if (
    runnable &&
    (facts.state !== 'prepared' ||
      facts.binding !== 'bound_attached' ||
      turns.some((t) => !turnEnded(t.state)))
  )
    throw invalid('hints');
  const partial = flag(v.partial, 'partial');
  const older = v.older === null ? null : parseHistoryCursor(v.older);
  // A history that was not read whole has somewhere older to look, always.
  if (partial && older === null) throw invalid('hints');
  return {
    schema_version: 3,
    head: v.head === null ? null : sequence(v.head, 'head'),
    session: facts,
    turns,
    runnable,
    partial,
    older,
  };
}

export function parseTurnHistory(value: unknown): TurnHistory {
  const v = record(
    value,
    ['schema_version', 'session', 'turns', 'older'],
    'history',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  const owner = session(v.session);
  if (!Array.isArray(v.turns) || v.turns.length > TURN_LIST_LIMIT)
    throw invalid('turns');
  const turns = v.turns.map(parseTurnSummary);
  // Every turn is the session's, each is listed once, and none of them is running: a turn
  // that has not ended is the newest of its session and belongs to the list, not to history.
  if (turns.some((t) => t.session.toLowerCase() !== owner.toLowerCase()))
    throw invalid('turns');
  if (new Set(turns.map((t) => t.id.toLowerCase())).size !== turns.length)
    throw invalid('turns');
  if (turns.some((t) => !turnEnded(t.state))) throw invalid('turns');
  return {
    schema_version: 1,
    session: owner,
    turns,
    older: v.older === null ? null : parseHistoryCursor(v.older),
  };
}

export function parseTurnReceipt(value: unknown): TurnReceipt {
  const v = record(value, ['id'], 'receipt');
  return { id: turnId(v.id) };
}
