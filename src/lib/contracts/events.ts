import {
  parseManagedApprovalId,
  parseManagedSessionId,
  parseManagedTurnId,
  type ManagedApprovalId,
  type ManagedSessionId,
  type ManagedTurnId,
} from './identities';

/**
 * The window's view of the authority's durable history: a projection made in Rust.
 *
 * A row is evidence that something was recorded. It carries a position, a time, a kind
 * from a closed vocabulary, Wyss's own identities of what it concerns and a few facts
 * from closed vocabularies. It carries no payload: nothing of the provider, no lease,
 * no fingerprint, no command, no output, no text anybody wrote. What a session, a turn
 * or an approval is *now* is never read from a row.
 *
 * Positions are opaque decimal text. They are compared for equality and handed back;
 * the only arithmetic done on them is exact (`BigInt`), to check that a page is the
 * unbroken run it claims to be. They are never numbers.
 */

export const EVENT_CATEGORIES = [
  'session',
  'runtime',
  'turn',
  'approval',
  'lease',
  'authority',
] as const;
export type EventCategory = (typeof EVENT_CATEGORIES)[number];

/** What was recorded, and the one category each kind belongs to. */
export const EVENT_KIND_CATEGORY = {
  session_prepared: 'session',
  session_closed: 'session',
  session_bound: 'session',
  session_attached: 'session',
  binding_indeterminate: 'session',
  binding_abandoned: 'session',
  runtime_desired: 'runtime',
  runtime_ready: 'runtime',
  runtime_exited: 'runtime',
  turn_admitted: 'turn',
  turn_running: 'turn',
  turn_cancel_requested: 'turn',
  turn_completed: 'turn',
  turn_interrupted: 'turn',
  turn_failed: 'turn',
  turn_indeterminate: 'turn',
  approval_requested: 'approval',
  approval_accepted: 'approval',
  approval_confirmed: 'approval',
  approval_declined: 'approval',
  approval_cancelled: 'approval',
  approval_expired: 'approval',
  approval_invalidated: 'approval',
  approval_indeterminate: 'approval',
  lease_granted: 'lease',
  lease_renewed: 'lease',
  lease_revoked: 'lease',
  lease_expired: 'lease',
  /** A kind this build does not know. Nothing of it is shown but where it is. */
  other: 'authority',
} as const satisfies Record<string, EventCategory>;
export type EventKind = keyof typeof EVENT_KIND_CATEGORY;

export const DISPOSITIONS = [
  'normal',
  'attention',
  'failure',
  'security',
] as const;
export type Disposition = (typeof DISPOSITIONS)[number];

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
export const APPROVAL_KINDS = ['command_execution', 'file_change'] as const;
export const APPROVAL_REASONS = [
  'client_decision',
  'approval_timeout',
  'lease_expired',
  'lease_revoked',
  'turn_cancelled',
  'turn_ended',
  'session_closed',
  'runtime_lost',
  'authority_shutdown',
  'authority_restarted',
  'effect_unknown',
  'unsupported',
  'isolation_violation',
] as const;
export const APPROVAL_EFFECTS = [
  'none',
  'dispatched',
  'confirmed',
  'unknown',
] as const;
export const DESIRED_STATES = ['running', 'stopped'] as const;
export const RUNTIME_EXITS = [
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
export const LEASE_SCOPES = [
  'session_create',
  'session_resume',
  'turn_start',
  'command_execute',
  'file_change',
] as const;
export const REVOCATIONS = ['user_request', 'session_closed'] as const;

export interface EventFacts {
  turn_reason: (typeof TURN_REASONS)[number] | null;
  approval_kind: (typeof APPROVAL_KINDS)[number] | null;
  approval_reason: (typeof APPROVAL_REASONS)[number] | null;
  approval_effect: (typeof APPROVAL_EFFECTS)[number] | null;
  desired: (typeof DESIRED_STATES)[number] | null;
  exit: (typeof RUNTIME_EXITS)[number] | null;
  expected: boolean | null;
  runtime_version: string | null;
  scopes: (typeof LEASE_SCOPES)[number][] | null;
  expires_at: string | null;
  revocation: (typeof REVOCATIONS)[number] | null;
}
export interface EventRow {
  /** Position in the stream: opaque decimal text. */
  sequence: string;
  at: string;
  category: EventCategory;
  kind: EventKind;
  disposition: Disposition;
  security: boolean;
  session: ManagedSessionId | null;
  turn: ManagedTurnId | null;
  approval: ManagedApprovalId | null;
  facts: EventFacts;
}
export interface EventsPage {
  schema_version: 1;
  /** The stream the events belong to, lower case. */
  stream: string;
  head: string | null;
  /** Newest first, without a hole. */
  events: EventRow[];
  /** Hand this back to read the window before this one. */
  older: string | null;
  /** History before the oldest event here is unavailable. */
  gap: boolean;
}

/** Events one page may hold. The desktop reads no more at once. */
export const EVENTS_PAGE_LIMIT = 50;

const SEQUENCE = /^[1-9][0-9]{0,19}$/;
const STREAM =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const TIMESTAMP =
  /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d{1,9})?([Zz]|\+00:00)$/;
const VERSION = /^[0-9A-Za-z.+-]{1,32}$/;
const FACTS = [
  'turn_reason',
  'approval_kind',
  'approval_reason',
  'approval_effect',
  'desired',
  'exit',
  'expected',
  'runtime_version',
  'scopes',
  'expires_at',
  'revocation',
] as const;

function invalid(field: string): Error {
  return new Error(`events: invalid_${field}`);
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
function member<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    throw invalid(field);
  return value as T;
}
function optional<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T | null {
  return value === null ? null : member(value, allowed, field);
}

function parseFacts(value: unknown): EventFacts {
  const v = record(value, FACTS, 'facts');
  if (v.expected !== null && typeof v.expected !== 'boolean')
    throw invalid('expected');
  if (
    v.runtime_version !== null &&
    (typeof v.runtime_version !== 'string' || !VERSION.test(v.runtime_version))
  )
    throw invalid('runtime_version');
  let scopes: EventFacts['scopes'] = null;
  if (v.scopes !== null) {
    if (!Array.isArray(v.scopes) || v.scopes.length > LEASE_SCOPES.length)
      throw invalid('scopes');
    scopes = v.scopes.map((s) => member(s, LEASE_SCOPES, 'scopes'));
    if (new Set(scopes).size !== scopes.length) throw invalid('scopes');
  }
  return {
    turn_reason: optional(v.turn_reason, TURN_REASONS, 'turn_reason'),
    approval_kind: optional(v.approval_kind, APPROVAL_KINDS, 'approval_kind'),
    approval_reason: optional(
      v.approval_reason,
      APPROVAL_REASONS,
      'approval_reason',
    ),
    approval_effect: optional(
      v.approval_effect,
      APPROVAL_EFFECTS,
      'approval_effect',
    ),
    desired: optional(v.desired, DESIRED_STATES, 'desired'),
    exit: optional(v.exit, RUNTIME_EXITS, 'exit'),
    expected: v.expected,
    runtime_version: v.runtime_version,
    scopes,
    expires_at:
      v.expires_at === null ? null : timestamp(v.expires_at, 'expires_at'),
    revocation: optional(v.revocation, REVOCATIONS, 'revocation'),
  };
}

export function parseEventRow(value: unknown): EventRow {
  const v = record(
    value,
    [
      'sequence',
      'at',
      'category',
      'kind',
      'disposition',
      'security',
      'session',
      'turn',
      'approval',
      'facts',
    ],
    'row',
  );
  const kind = member(
    v.kind,
    Object.keys(EVENT_KIND_CATEGORY) as EventKind[],
    'kind',
  );
  const category = member(v.category, EVENT_CATEGORIES, 'category');
  // A kind belongs to one category: a row that says otherwise is not the projection.
  if (EVENT_KIND_CATEGORY[kind] !== category) throw invalid('category');
  if (typeof v.security !== 'boolean') throw invalid('security');
  const disposition = member(v.disposition, DISPOSITIONS, 'disposition');
  const facts = parseFacts(v.facts);
  const session = v.session === null ? null : parseManagedSessionId(v.session);
  const turn = v.turn === null ? null : parseManagedTurnId(v.turn);
  const approval =
    v.approval === null ? null : parseManagedApprovalId(v.approval);
  // Identities only where the event is about them, and nothing at all for a kind this
  // build does not know.
  if (turn !== null && category !== 'turn' && category !== 'approval')
    throw invalid('turn');
  if (approval !== null && category !== 'approval') throw invalid('approval');
  if (
    kind === 'other' &&
    (disposition !== 'normal' ||
      v.security ||
      FACTS.some((name) => facts[name] !== null))
  )
    throw invalid('other');
  return {
    sequence: sequence(v.sequence, 'sequence'),
    at: timestamp(v.at, 'at'),
    category,
    kind,
    disposition,
    security: v.security,
    session,
    turn,
    approval,
    facts,
  };
}

export function parseEventsPage(value: unknown): EventsPage {
  const v = record(
    value,
    ['schema_version', 'stream', 'head', 'events', 'older', 'gap'],
    'page',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  if (typeof v.stream !== 'string' || !STREAM.test(v.stream))
    throw invalid('stream');
  if (typeof v.gap !== 'boolean') throw invalid('gap');
  if (!Array.isArray(v.events) || v.events.length > EVENTS_PAGE_LIMIT)
    throw invalid('events');
  const events = v.events.map(parseEventRow);
  const head = v.head === null ? null : sequence(v.head, 'head');
  const older = v.older === null ? null : sequence(v.older, 'older');
  // Newest first and without a hole: each event is exactly one position before the
  // one above it. Exact arithmetic on the text, never a number.
  for (let i = 1; i < events.length; i++) {
    if (BigInt(events[i - 1].sequence) - BigInt(events[i].sequence) !== 1n)
      throw invalid('order');
  }
  const newest = events.at(0)?.sequence;
  const oldest = events.at(-1)?.sequence;
  // Nothing is newer than the head that was read before it.
  if (newest !== undefined && (head === null || BigInt(newest) > BigInt(head)))
    throw invalid('head');
  // The position to hand back is the oldest event shown, and there is none to hand
  // back at the beginning of a stream or at a hole.
  if (older !== null && (older !== oldest || older === '1' || v.gap))
    throw invalid('older');
  if (older === null && oldest !== undefined && oldest !== '1' && !v.gap)
    throw invalid('older');
  if (v.gap && events.length === 0) throw invalid('gap');
  return {
    schema_version: 1,
    stream: v.stream.toLowerCase(),
    head,
    events,
    older,
    gap: v.gap,
  };
}
