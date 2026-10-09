import { invoke } from '@tauri-apps/api/core';
import {
  parseApprovalDetail,
  parseApprovalList,
  parseDecisionReceipt,
  type ApprovalDetail,
  type ApprovalList,
  type DecisionReceipt,
} from '../contracts/approvals';
import type { ManagedApprovalId } from '../contracts/identities';
import { parseSessionFailure } from '../contracts/sessions';
import { SessionCallError } from './sessions-client';

/**
 * The approval calls the window can make: two reads and two decisions. A decision is
 * the command it is sent to: neither takes a decision as data, a command to run
 * instead, or anything that outlives the one approval it names.
 */
async function call<T>(
  command:
    | 'managed_approvals'
    | 'managed_approval_read'
    | 'managed_approval_approve'
    | 'managed_approval_deny',
  request: Record<string, unknown>,
  parse: (value: unknown) => T,
): Promise<T> {
  let answer: unknown;
  try {
    answer = await invoke<unknown>(command, { request });
  } catch (error) {
    throw new SessionCallError(parseSessionFailure(error));
  }
  try {
    return parse(answer);
  } catch {
    // An answer that is not the projection is a failed bridge, never data.
    throw new SessionCallError({ code: 'internal', retry: false });
  }
}

/** The latest approvals: those waiting for a decision, with what they are about. */
export function readApprovals(): Promise<ApprovalList> {
  return call('managed_approvals', {}, parseApprovalList);
}

/**
 * One approval as the authority holds it now. `file` and `offset` select a file of a
 * change set and a page of its diff; null is the first of each.
 */
export function readApproval(
  approvalId: ManagedApprovalId,
  file: number | null,
  offset: number | null,
): Promise<ApprovalDetail> {
  return call(
    'managed_approval_read',
    { approval_id: approvalId, file, offset },
    (value) => {
      const detail = parseApprovalDetail(value);
      // An answer about another approval, file or page is not an answer to this one.
      if (detail.approval.id.toLowerCase() !== approvalId.toLowerCase())
        throw new Error('approval: other_approval');
      if (
        detail.file !== null &&
        ((file !== null && detail.file.index !== file) ||
          detail.file.offset !== (offset ?? 0))
      )
        throw new Error('approval: other_page');
      return detail;
    },
  );
}

function decide(
  command: 'managed_approval_approve' | 'managed_approval_deny',
  intentId: string,
  approvalId: ManagedApprovalId,
  expectedRevision: number,
): Promise<DecisionReceipt> {
  return call(
    command,
    {
      intent_id: intentId,
      approval_id: approvalId,
      expected_revision: expectedRevision,
    },
    (value) => {
      const receipt = parseDecisionReceipt(value);
      if (receipt.id.toLowerCase() !== approvalId.toLowerCase())
        throw new Error('approval: other_receipt');
      return receipt;
    },
  );
}
/** `intentId` names the user's action: sent again unchanged, it is the same decision. */
export function approveApproval(
  intentId: string,
  approvalId: ManagedApprovalId,
  expectedRevision: number,
): Promise<DecisionReceipt> {
  return decide(
    'managed_approval_approve',
    intentId,
    approvalId,
    expectedRevision,
  );
}
export function denyApproval(
  intentId: string,
  approvalId: ManagedApprovalId,
  expectedRevision: number,
): Promise<DecisionReceipt> {
  return decide(
    'managed_approval_deny',
    intentId,
    approvalId,
    expectedRevision,
  );
}
