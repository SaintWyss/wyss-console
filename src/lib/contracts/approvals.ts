/**
 * ApprovalList v1 and ApprovalDetail: the desktop's projection of the approvals of
 * managed turns. It is not the control contract. An approval carries its own identity,
 * as a handle to read and answer it with, its session and turn, and the action it is
 * about while the authority still holds it: a command with its directory, or one file
 * of a change set with one page of its diff. No provider item, lease, runtime instance,
 * fingerprint or effect marker has a place in it.
 */
import {
  parseManagedApprovalId,
  parseManagedSessionId,
  parseManagedTurnId,
  type ManagedApprovalId,
  type ManagedSessionId,
  type ManagedTurnId,
} from './identities';
import { APPROVAL_KINDS, type ApprovalKind } from './turns';

/** The authority's own names. Only `pending` is not final. */
export const APPROVAL_STATES = [
  'pending',
  'accepted',
  'declined',
  'cancelled',
  'expired',
  'invalidated',
  'indeterminate',
] as const;
export type ApprovalState = (typeof APPROVAL_STATES)[number];
/** Why an approval ended, as the authority recorded it. */
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
export type ApprovalReason = (typeof APPROVAL_REASONS)[number];
export const APPROVAL_DECISIONS = ['accept', 'decline', 'cancel'] as const;
export type ApprovalDecision = (typeof APPROVAL_DECISIONS)[number];
export const FILE_CHANGE_KINDS = ['add', 'delete', 'update', 'move'] as const;
export type FileChangeKind = (typeof FILE_CHANGE_KINDS)[number];

export interface ApprovalSummary {
  /** A handle: it names this approval when it is read or answered, and nothing else. */
  id: ManagedApprovalId;
  session: ManagedSessionId;
  turn: ManagedTurnId;
  kind: ApprovalKind;
  state: ApprovalState;
  revision: number;
  requested_at: string;
  expires_at: string;
  ended_at: string | null;
  reason: ApprovalReason | null;
  decision: ApprovalDecision | null;
  /** Presentation hint: pending, with its action still held. The authority decides. */
  decidable: boolean;
}
/** The command under approval, whole, as the authority presents it. */
export interface CommandView {
  command: string;
  cwd: string;
}
/** One file of the change set under approval, with one page of its diff. */
export interface FileView {
  index: number;
  path: string;
  kind: FileChangeKind;
  move_path: string | null;
  /** Size of the whole diff of this file, in bytes. */
  bytes: number;
  /** Where `chunk` starts in it. */
  offset: number;
  chunk: string;
  /** Where the next page starts. Null when this one ends the diff. */
  next: number | null;
}
export interface ApprovalDetail {
  approval: ApprovalSummary;
  workspace: string | null;
  command: CommandView | null;
  /** How many files the change set has. */
  files: number | null;
  file: FileView | null;
}
export interface ApprovalList {
  schema_version: 1;
  head: string | null;
  /** Waiting for a decision, with their action. */
  pending: ApprovalDetail[];
  /** Ended, newest first. Their action is no longer held. */
  recent: ApprovalSummary[];
  partial: boolean;
}
export interface DecisionReceipt {
  id: ManagedApprovalId;
  state: ApprovalState;
}

/** Approvals one list may hold. The desktop reads no more. */
export const APPROVAL_LIST_LIMIT = 8;
/** The authority's own bounds for what it presents. */
export const COMMAND_MAX_BYTES = 16 * 1024;
const CWD_MAX_BYTES = 4 * 1024;
const PATH_MAX_BYTES = 1024;
export const DIFF_PAGE_BYTES = 8 * 1024;
export const FILE_DIFF_MAX_BYTES = 128 * 1024;
export const FILES_MAX = 64;

const SEQUENCE = /^[1-9][0-9]{0,19}$/;
const TIMESTAMP =
  /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d{1,9})?([Zz]|\+00:00)$/;
const encoder = new TextEncoder();
const bytes = (text: string) => encoder.encode(text).length;

function invalid(field: string): Error {
  return new Error(`approval: invalid_${field}`);
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
function timestamp(value: unknown, field: string): string {
  if (
    typeof value !== 'string' ||
    !TIMESTAMP.test(value) ||
    Number.isNaN(Date.parse(value))
  )
    throw invalid(field);
  return value;
}
function count(value: unknown, max: number, field: string): number {
  if (
    !Number.isSafeInteger(value) ||
    (value as number) < 0 ||
    (value as number) > max
  )
    throw invalid(field);
  return value as number;
}
/** Text of bounded size with no NUL. What it says is never interpreted. */
function text(value: unknown, max: number, field: string): string {
  if (
    typeof value !== 'string' ||
    value.includes('\u0000') ||
    bytes(value) > max
  )
    throw invalid(field);
  return value;
}
/** An absolute path, as the authority keeps it. */
function folder(value: unknown, max: number, field: string): string {
  const path = name(value, max, field);
  if (!path.startsWith('/')) throw invalid(field);
  return path;
}
/** A path as the authority presents it: one line of text that names something. */
function name(value: unknown, max: number, field: string): string {
  const path = text(value, max, field);
  // eslint-disable-next-line no-control-regex
  if (path === '' || /[\u0000-\u001f\u007f-\u009f]/.test(path))
    throw invalid(field);
  return path;
}

export function parseApprovalSummary(value: unknown): ApprovalSummary {
  const v = record(
    value,
    [
      'id',
      'session',
      'turn',
      'kind',
      'state',
      'revision',
      'requested_at',
      'expires_at',
      'ended_at',
      'reason',
      'decision',
      'decidable',
    ],
    'summary',
  );
  let id: ManagedApprovalId;
  let session: ManagedSessionId;
  let turn: ManagedTurnId;
  try {
    id = parseManagedApprovalId(v.id);
    session = parseManagedSessionId(v.session);
    turn = parseManagedTurnId(v.turn);
  } catch {
    throw invalid('identity');
  }
  const state = member(v.state, APPROVAL_STATES, 'state');
  const pending = state === 'pending';
  // An approval that ended says when and why; one that has not, says neither.
  if ((v.ended_at === null) !== pending || (v.reason === null) !== pending)
    throw invalid('end');
  const decision =
    v.decision === null
      ? null
      : member(v.decision, APPROVAL_DECISIONS, 'decision');
  // What was decided is what the state says was decided.
  const decided: Partial<Record<ApprovalState, ApprovalDecision>> = {
    accepted: 'accept',
    declined: 'decline',
    cancelled: 'cancel',
    indeterminate: 'accept',
  };
  if ((decided[state] ?? null) !== decision) throw invalid('decision');
  const decidable = flag(v.decidable, 'decidable');
  // No hint offers a decision the shown state already rules out.
  if (decidable && !pending) throw invalid('hints');
  if (!Number.isSafeInteger(v.revision) || (v.revision as number) < 1)
    throw invalid('revision');
  return {
    id,
    session,
    turn,
    kind: member(v.kind, APPROVAL_KINDS, 'kind'),
    state,
    revision: v.revision as number,
    requested_at: timestamp(v.requested_at, 'requested_at'),
    expires_at: timestamp(v.expires_at, 'expires_at'),
    ended_at: v.ended_at === null ? null : timestamp(v.ended_at, 'ended_at'),
    reason:
      v.reason === null ? null : member(v.reason, APPROVAL_REASONS, 'reason'),
    decision,
    decidable,
  };
}

function parseCommand(value: unknown): CommandView {
  const v = record(value, ['command', 'cwd'], 'command');
  const command = text(v.command, COMMAND_MAX_BYTES, 'command');
  if (command === '') throw invalid('command');
  return { command, cwd: folder(v.cwd, CWD_MAX_BYTES, 'cwd') };
}
function parseFile(value: unknown): FileView {
  const v = record(
    value,
    ['index', 'path', 'kind', 'move_path', 'bytes', 'offset', 'chunk', 'next'],
    'file',
  );
  const size = count(v.bytes, FILE_DIFF_MAX_BYTES, 'bytes');
  const offset = count(v.offset, size, 'offset');
  const chunk = text(v.chunk, DIFF_PAGE_BYTES, 'chunk');
  const next = v.next === null ? null : count(v.next, size, 'next');
  // A page lies inside its diff, and the next one starts exactly where it ends: a next
  // page exists only when something of the diff is left for it.
  const end = offset + bytes(chunk);
  if (
    end > size ||
    (next === null
      ? end !== size
      : next !== end || next >= size || chunk === '')
  )
    throw invalid('page');
  const kind = member(v.kind, FILE_CHANGE_KINDS, 'file_kind');
  const moved =
    v.move_path === null
      ? null
      : name(v.move_path, PATH_MAX_BYTES, 'move_path');
  if ((moved !== null) !== (kind === 'move')) throw invalid('move_path');
  return {
    index: count(v.index, FILES_MAX - 1, 'index'),
    path: name(v.path, PATH_MAX_BYTES, 'path'),
    kind,
    move_path: moved,
    bytes: size,
    offset,
    chunk,
    next,
  };
}

/** Validates one approval whole. Anything unexpected is an error, never a guess. */
export function parseApprovalDetail(value: unknown): ApprovalDetail {
  const v = record(
    value,
    ['approval', 'workspace', 'command', 'files', 'file'],
    'detail',
  );
  const approval = parseApprovalSummary(v.approval);
  const command = v.command === null ? null : parseCommand(v.command);
  const files = v.files === null ? null : count(v.files, FILES_MAX, 'files');
  const file = v.file === null ? null : parseFile(v.file);
  // The action is of the approval's own kind, and a file is one of its change set.
  if (
    (command !== null && approval.kind !== 'command_execution') ||
    (files !== null && approval.kind !== 'file_change') ||
    (command !== null && files !== null) ||
    files === 0 ||
    (file !== null && (files === null || file.index >= files))
  )
    throw invalid('action');
  // Only a pending approval has an action to show, and one that can be decided has one.
  if (approval.state !== 'pending' && (command !== null || files !== null))
    throw invalid('action');
  if (approval.decidable && command === null && files === null)
    throw invalid('action');
  return {
    approval,
    workspace:
      v.workspace === null ? null : folder(v.workspace, 256, 'workspace'),
    command,
    files,
    file,
  };
}

export function parseApprovalList(value: unknown): ApprovalList {
  const v = record(
    value,
    ['schema_version', 'head', 'pending', 'recent', 'partial'],
    'list',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  if (
    !Array.isArray(v.pending) ||
    !Array.isArray(v.recent) ||
    v.pending.length + v.recent.length > APPROVAL_LIST_LIMIT
  )
    throw invalid('list');
  const pending = v.pending.map(parseApprovalDetail);
  const recent = v.recent.map(parseApprovalSummary);
  // Each list holds what it says it holds, and no approval twice.
  if (
    pending.some((p) => p.approval.state !== 'pending') ||
    recent.some((r) => r.state === 'pending')
  )
    throw invalid('list');
  const ids = [
    ...pending.map((p) => p.approval.id),
    ...recent.map((r) => r.id),
  ];
  if (new Set(ids.map((id) => id.toLowerCase())).size !== ids.length)
    throw invalid('list');
  if (v.head !== null && (typeof v.head !== 'string' || !SEQUENCE.test(v.head)))
    throw invalid('head');
  return {
    schema_version: 1,
    head: v.head as string | null,
    pending,
    recent,
    partial: flag(v.partial, 'partial'),
  };
}

export function parseDecisionReceipt(value: unknown): DecisionReceipt {
  const v = record(value, ['id', 'state'], 'receipt');
  let id: ManagedApprovalId;
  try {
    id = parseManagedApprovalId(v.id);
  } catch {
    throw invalid('receipt');
  }
  const state = member(v.state, APPROVAL_STATES, 'state');
  // A decision the authority committed is never a pending approval.
  if (state === 'pending') throw invalid('receipt');
  return { id, state };
}
