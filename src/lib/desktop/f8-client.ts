import { invoke } from '@tauri-apps/api/core';
import {
  parseAbandonReceipt,
  parseAuditInfo,
  parseBindingReceipt,
  parseDecisionReceipt,
  parseF8Failure,
  parseF8State,
  parseLeaseReceipt,
  parseModelsInfo,
  parseRuntimeReceipt,
  parseSessionDetail,
  parseSessionReceipt,
  parseSessionsView,
  parseTurnReading,
  parseTurnReceipt,
  parseWatchInfo,
  type AbandonReceipt,
  type AuditInfo,
  type BindingReceipt,
  type DecisionReceipt,
  type F8Failure,
  type F8State,
  type Fence,
  type LeaseReceipt,
  type LeaseScope,
  type ModelsInfo,
  type RuntimeReceipt,
  type SessionDetail,
  type SessionReceipt,
  type SessionsView,
  type TurnReading,
  type TurnReceipt,
  type WatchInfo,
} from '../contracts/f8';
import {
  parseApprovalDetail,
  type ApprovalDetail,
} from '../contracts/approvals';
import type {
  ManagedApprovalId,
  ManagedSessionId,
  ManagedTurnId,
} from '../contracts/identities';
import type { AgentMode } from '../contracts/sessions';

/** A protocol-2 call the desktop refused or could not complete, in product terms. */
export class F8CallError extends Error {
  constructor(readonly failure: F8Failure) {
    super(`f8: ${failure.code}`);
  }
}

/**
 * The calls the window can make of a protocol-2 authority. None names a control method:
 * the window sends a session, turn or approval it was shown, the fence and the revision it
 * saw, the identity of its own action and, where the product offers a choice, one of a
 * closed set. A decision on an approval is three commands, never a field.
 */
async function call<T>(
  command: string,
  request: Record<string, unknown> | null,
  parse: (value: unknown) => T,
): Promise<T> {
  let answer: unknown;
  try {
    answer =
      request === null
        ? await invoke<unknown>(command)
        : await invoke<unknown>(command, { request });
  } catch (error) {
    throw new F8CallError(parseF8Failure(error));
  }
  try {
    return parse(answer);
  } catch {
    // An answer that is not the projection is a failed bridge, never data.
    throw new F8CallError({
      code: 'internal',
      retry: false,
      retry_in_ms: null,
    });
  }
}

/** What the interface is and how its link stands. An answer that is not one means inactive. */
export function readF8State(): Promise<F8State> {
  return call('f8_state', null, parseF8State);
}
export function reconnectF8(): Promise<F8State> {
  return call('f8_reconnect', null, parseF8State);
}
export function readSessions(before: string | null): Promise<SessionsView> {
  return call('f8_sessions', { before }, parseSessionsView);
}
export function readSession(session: ManagedSessionId): Promise<SessionDetail> {
  return call('f8_session', { session_id: session }, parseSessionDetail);
}
/** `intentId` names the user's action: sent again unchanged, it is the same action. */
export function prepareSession(intentId: string): Promise<SessionReceipt> {
  return call(
    'f8_session_prepare',
    { intent_id: intentId },
    parseSessionReceipt,
  );
}
export function closeSession(
  intentId: string,
  fence: Fence,
  revision: number,
): Promise<SessionReceipt> {
  return call(
    'f8_session_close',
    { intent_id: intentId, fence, expected_revision: revision },
    parseSessionReceipt,
  );
}
export function startRuntime(
  intentId: string,
  session: ManagedSessionId,
  revision: number,
  generation: number,
  mode: AgentMode,
  workspace: string | null,
): Promise<RuntimeReceipt> {
  return call(
    'f8_runtime_start',
    {
      intent_id: intentId,
      session_id: session,
      expected_revision: revision,
      expected_generation: generation,
      mode,
      workspace,
    },
    parseRuntimeReceipt,
  );
}
export function stopRuntime(
  intentId: string,
  fence: Fence,
  revision: number,
): Promise<RuntimeReceipt> {
  return call(
    'f8_runtime_stop',
    { intent_id: intentId, fence, expected_revision: revision },
    parseRuntimeReceipt,
  );
}
export function readModels(fence: Fence): Promise<ModelsInfo> {
  return call('f8_models', { fence }, parseModelsInfo);
}
export function grantLease(
  intentId: string,
  fence: Fence,
  sessionRevision: number,
  scopes: readonly LeaseScope[],
  ttlSeconds: number,
): Promise<LeaseReceipt> {
  return call(
    'f8_lease_grant',
    {
      intent_id: intentId,
      fence,
      expected_session_revision: sessionRevision,
      scopes,
      ttl_seconds: ttlSeconds,
    },
    parseLeaseReceipt,
  );
}
export function renewLease(
  intentId: string,
  fence: Fence,
  lease: string,
  revision: number,
  ttlSeconds: number,
): Promise<LeaseReceipt> {
  return call(
    'f8_lease_renew',
    {
      intent_id: intentId,
      fence,
      lease_id: lease,
      expected_revision: revision,
      ttl_seconds: ttlSeconds,
    },
    parseLeaseReceipt,
  );
}
export function revokeLease(
  intentId: string,
  lease: string,
  revision: number,
): Promise<LeaseReceipt> {
  return call(
    'f8_lease_revoke',
    { intent_id: intentId, lease_id: lease, expected_revision: revision },
    parseLeaseReceipt,
  );
}
export function createBinding(
  intentId: string,
  fence: Fence,
  sessionRevision: number,
  workspace: string,
): Promise<BindingReceipt> {
  return call(
    'f8_binding_create',
    {
      intent_id: intentId,
      fence,
      expected_session_revision: sessionRevision,
      workspace,
    },
    parseBindingReceipt,
  );
}
export function resumeBinding(
  intentId: string,
  fence: Fence,
  sessionRevision: number,
): Promise<BindingReceipt> {
  return call(
    'f8_binding_resume',
    {
      intent_id: intentId,
      fence,
      expected_session_revision: sessionRevision,
    },
    parseBindingReceipt,
  );
}
export function abandonBinding(
  intentId: string,
  fence: Fence,
  operation: string,
): Promise<AbandonReceipt> {
  return call(
    'f8_binding_abandon',
    { intent_id: intentId, fence, operation_id: operation },
    parseAbandonReceipt,
  );
}
export function startTurn(
  intentId: string,
  fence: Fence,
  sessionRevision: number,
  prompt: string,
): Promise<TurnReceipt> {
  return call(
    'f8_turn_start',
    {
      intent_id: intentId,
      fence,
      expected_session_revision: sessionRevision,
      prompt,
    },
    parseTurnReceipt,
  );
}
export function cancelTurn(
  intentId: string,
  fence: Fence,
  turn: ManagedTurnId | string,
  revision: number,
): Promise<TurnReceipt> {
  return call(
    'f8_turn_cancel',
    {
      intent_id: intentId,
      fence,
      turn_id: turn,
      expected_revision: revision,
    },
    parseTurnReceipt,
  );
}
/** One reading of a turn; with `wait`, the desktop holds the answer while nothing is new. */
export function readTurn(
  turn: ManagedTurnId | string,
  after: string | null,
  wait: boolean,
): Promise<TurnReading> {
  return call('f8_turn_read', { turn_id: turn, after, wait }, (value) => {
    const reading = parseTurnReading(value);
    if (reading.turn.turn.id.toLowerCase() !== String(turn).toLowerCase())
      throw new Error('f8: other_turn');
    // Nothing at or before the cursor comes back, and the cursor never goes back.
    const from = after === null ? 0n : BigInt(after);
    if (
      reading.events.some((event) => BigInt(event.sequence) <= from) ||
      (reading.next !== null && BigInt(reading.next) < from)
    )
      throw new Error('f8: cursor');
    return reading;
  });
}
export function readApproval(
  approval: ManagedApprovalId | string,
  file: number | null,
  offset: number | null,
): Promise<ApprovalDetail> {
  return call(
    'f8_approval_read',
    { approval_id: approval, file, offset },
    (value) => {
      const detail = parseApprovalDetail(value);
      if (detail.approval.id.toLowerCase() !== String(approval).toLowerCase())
        throw new Error('f8: other_approval');
      return detail;
    },
  );
}

/** The three decisions the window can ask for. Which one is fixed by the command. */
export type Decision = 'accept' | 'decline' | 'cancel';
const DECISION_COMMAND: Record<Decision, string> = {
  accept: 'f8_approval_accept',
  decline: 'f8_approval_decline',
  cancel: 'f8_approval_cancel',
};
export function decideApproval(
  decision: Decision,
  intentId: string,
  fence: Fence,
  approval: ManagedApprovalId | string,
  revision: number,
): Promise<DecisionReceipt> {
  return call(
    DECISION_COMMAND[decision],
    {
      intent_id: intentId,
      fence,
      approval_id: approval,
      expected_revision: revision,
    },
    (value) => {
      const done = parseDecisionReceipt(value);
      if (done.id.toLowerCase() !== String(approval).toLowerCase())
        throw new Error('f8: other_approval');
      return done;
    },
  );
}

/** The newest window of the durable history, or the one before a position of a stream. */
export function readAudit(
  older: { stream_id: string; before: string } | null,
): Promise<AuditInfo> {
  return call('f8_audit', { older }, parseAuditInfo);
}
/** Waits for the next durable commit after `after`, for at most `waitMs`. */
export function waitAudit(
  stream: string,
  after: string | null,
  waitMs: number,
): Promise<WatchInfo> {
  return call(
    'f8_audit_wait',
    { stream_id: stream, after, wait_ms: waitMs },
    parseWatchInfo,
  );
}
