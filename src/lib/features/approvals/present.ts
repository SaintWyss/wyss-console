import type {
  ApprovalReason,
  ApprovalState,
  FileChangeKind,
} from '../../contracts/approvals';
import type { SessionFailureCode } from '../../contracts/sessions';
import type { ApprovalKind } from '../../contracts/turns';
import type { RuntimeTone } from '../../shell/runtime';
import type { ApprovalCard, DecisionKind } from './controller';

/** Presentation only: nothing here is read back as state. */

export const APPROVAL_STATE: Record<
  ApprovalState,
  { label: string; tone: RuntimeTone }
> = {
  pending: { label: 'Pending', tone: 'pending' },
  accepted: { label: 'Approved', tone: 'ok' },
  declined: { label: 'Denied', tone: 'idle' },
  cancelled: { label: 'Cancelled', tone: 'idle' },
  expired: { label: 'Expired', tone: 'warn' },
  invalidated: { label: 'No longer applies', tone: 'idle' },
  indeterminate: { label: 'Outcome unknown', tone: 'warn' },
};

export const APPROVAL_KIND: Record<ApprovalKind, string> = {
  command_execution: 'Command',
  file_change: 'File change',
};

/** Why an approval ended, in one sentence. Empty when the state already says it. */
export const APPROVAL_REASON: Record<ApprovalReason, string> = {
  client_decision: '',
  approval_timeout: 'Nobody decided it in time.',
  lease_expired: 'The execution lease of its turn expired.',
  lease_revoked: 'The execution lease of its turn was revoked.',
  turn_cancelled: 'Its turn was cancelled.',
  turn_ended: 'Its turn ended.',
  session_closed: 'Its session was closed.',
  runtime_lost: 'Codex stopped while it waited.',
  authority_shutdown: 'The authority shut down while it waited.',
  authority_restarted: 'The authority restarted while it waited.',
  effect_unknown:
    'It was approved, and the authority could not confirm what Codex did with that.',
  unsupported: 'Codex asked for something that cannot be approved.',
  isolation_violation: 'Codex acted outside what the session allows.',
};

export const FILE_KIND: Record<FileChangeKind, string> = {
  add: 'Add',
  delete: 'Delete',
  update: 'Modify',
  move: 'Move',
};

/** What is being asked, in one line. It names the kind of action, never guesses at it. */
export function approvalTitle(card: ApprovalCard): string {
  if (card.approval.kind === 'command_execution')
    return 'Codex wants to run a command';
  if (card.files === null) return 'Codex wants to change files';
  return card.files === 1
    ? 'Codex wants to change a file'
    : `Codex wants to change ${card.files} files`;
}

/** The last segment of the workspace, or a short form of the session's identity. */
export function approvalWhere(
  workspace: string | null,
  session: string,
): string {
  const name = workspace?.split('/').filter(Boolean).at(-1);
  return name ?? `Session ${session.slice(0, 8)}`;
}

/**
 * Whether everything the approval is about was read: for a change set, every file and
 * the whole diff of each. Approve is offered only then. It is a presentation rule: the
 * authority checks the action again, whatever was shown.
 */
export function wholeShown(card: ApprovalCard): boolean {
  if (card.command !== null) return true;
  if (card.files === null) return false;
  return (
    card.shown.length === card.files && card.shown.every((f) => f.next === null)
  );
}

const DOING: Record<DecisionKind, string> = {
  approve: 'Asking the authority to approve it…',
  deny: 'Asking the authority to deny it…',
};
export function decidingMessage(kind: DecisionKind): string {
  return DOING[kind];
}

const NOT: Record<DecisionKind, string> = {
  approve: 'It was not approved',
  deny: 'It was not denied',
};
const DECISION: Partial<Record<SessionFailureCode, string>> = {
  invalid: 'The console sent a request it should not have. Nothing changed.',
  not_found: 'The authority no longer has this approval.',
  changed: 'The approval changed after it was shown. It has been read again.',
  not_allowed: 'The approval had already ended.',
  permission_denied:
    'The authority denied it: the lease its turn was admitted under does not allow this action.',
  runtime_unavailable: 'Codex is no longer there to answer.',
  busy: 'The authority is busy with another operation. Try again in a moment.',
  uncertain:
    'The authority could not confirm what Codex did with the answer. Nothing is sent again by itself.',
  conflict: 'That request was already used for another decision.',
  incompatible: 'This console and the authority speak different versions.',
};
/** One sentence for a decision that failed. An unknown outcome is never a refusal. */
export function decisionFailureMessage(
  kind: DecisionKind,
  code: SessionFailureCode,
): string {
  if (code === 'unavailable')
    return 'The authority did not answer, so it is not known whether the decision was taken. Retrying repeats the same decision: it cannot decide twice.';
  return `${NOT[kind]}. ${DECISION[code] ?? 'Something unexpected happened. Nothing is assumed to have changed.'}`;
}

export function approvalsReadFailure(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? 'Approvals could not be read: the authority did not answer.'
    : code === 'busy'
      ? 'Approvals could not be read. The authority is busy; try again in a moment.'
      : 'Approvals could not be read. Nothing is shown rather than something unverified.';
}

/** How much of a diff is shown, when it is not all of it. */
export function shownOf(text: string, bytes: number): string {
  return `${new TextEncoder().encode(text).length.toLocaleString()} of ${bytes.toLocaleString()} bytes shown`;
}
