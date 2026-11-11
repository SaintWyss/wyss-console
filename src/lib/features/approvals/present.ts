import type {
  ApprovalReason,
  ApprovalState,
  FileChangeKind,
} from '../../contracts/approvals';
import type { SessionFailureCode } from '../../contracts/sessions';
import type { ApprovalKind } from '../../contracts/turns';
import type { RuntimeTone } from '../../shell/runtime';
import { folderName, shortId } from '../sessions/present';
import type { ApprovalCard, DecisionKind } from './controller';
import { msg } from '../../i18n/msg';

export interface MessageFact {
  text: string;
  values?: Record<string, string | number>;
  translate?: boolean;
}

/** Presentation only: nothing here is read back as state. */

export const APPROVAL_STATE: Record<
  ApprovalState,
  { label: string; tone: RuntimeTone }
> = {
  pending: { label: msg('Pending'), tone: 'pending' },
  accepted: { label: msg('Approved'), tone: 'ok' },
  declined: { label: msg('Denied'), tone: 'idle' },
  cancelled: { label: msg('Cancelled'), tone: 'idle' },
  expired: { label: msg('Expired'), tone: 'warn' },
  invalidated: { label: msg('No longer applies'), tone: 'idle' },
  indeterminate: { label: msg('Outcome unknown'), tone: 'warn' },
};

export const APPROVAL_KIND: Record<ApprovalKind, string> = {
  command_execution: msg('Command'),
  file_change: msg('File change'),
};

/** Why an approval ended, in one sentence. Empty when the state already says it. */
export const APPROVAL_REASON: Record<ApprovalReason, string> = {
  client_decision: '',
  approval_timeout: msg('Nobody decided it in time.'),
  lease_expired: msg('The execution lease of its turn expired.'),
  lease_revoked: msg('The execution lease of its turn was revoked.'),
  turn_cancelled: msg('Its turn was cancelled.'),
  turn_ended: msg('Its turn ended.'),
  session_closed: msg('Its session was closed.'),
  runtime_lost: msg('Codex stopped while it waited.'),
  authority_shutdown: msg('The authority shut down while it waited.'),
  authority_restarted: msg('The authority restarted while it waited.'),
  effect_unknown: msg(
    'It was approved, and the authority could not confirm what Codex did with that.',
  ),
  unsupported: msg('Codex asked for something that cannot be approved.'),
  isolation_violation: msg('Codex acted outside what the session allows.'),
};

export const FILE_KIND: Record<FileChangeKind, string> = {
  add: msg('Add'),
  delete: msg('Delete'),
  update: msg('Modify'),
  move: msg('Move'),
};

/** What is being asked, in one line. It names the kind of action, never guesses at it. */
export function approvalTitle(card: ApprovalCard): MessageFact {
  if (card.approval.kind === 'command_execution')
    return { text: msg('Codex wants to run a command') };
  if (card.files === null) return { text: msg('Codex wants to change files') };
  return card.files === 1
    ? { text: msg('Codex wants to change a file') }
    : {
        text: msg('Codex wants to change {count} files'),
        values: { count: card.files },
      };
}

/** The last segment of the workspace, or a short form of the session's identity. */
export function approvalWhere(
  workspace: string | null,
  session: string,
): MessageFact {
  const folder = folderName(workspace);
  return folder
    ? { text: folder, translate: false }
    : {
        text: msg('Session {id}'),
        values: { id: shortId(session) },
      };
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
  approve: msg('Asking the authority to approve it…'),
  deny: msg('Asking the authority to deny it…'),
};
export function decidingMessage(kind: DecisionKind): string {
  return DOING[kind];
}

const NOT: Record<DecisionKind, string> = {
  approve: msg('It was not approved.'),
  deny: msg('It was not denied.'),
};
const DECISION: Partial<Record<SessionFailureCode, string>> = {
  invalid: msg(
    'The console sent a request it should not have. Nothing changed.',
  ),
  not_found: msg('The authority no longer has this approval.'),
  changed: msg(
    'The approval changed after it was shown. It has been read again.',
  ),
  not_allowed: msg('The approval had already ended.'),
  permission_denied: msg(
    'The authority denied it: the lease its turn was admitted under does not allow this action.',
  ),
  runtime_unavailable: msg('Codex is no longer there to answer.'),
  busy: msg(
    'The authority is busy with another operation. Try again in a moment.',
  ),
  uncertain: msg(
    'The authority could not confirm what Codex did with the answer. Nothing is sent again by itself.',
  ),
  conflict: msg('That request was already used for another decision.'),
  incompatible: msg('This console and the authority speak different versions.'),
};
/** One sentence for a decision that failed. An unknown outcome is never a refusal. */
export function decisionFailureMessage(
  kind: DecisionKind,
  code: SessionFailureCode,
): MessageFact[] {
  if (code === 'unavailable')
    return [
      {
        text: msg(
          'The authority did not answer, so it is not known whether the decision was taken. Retrying repeats the same decision: it cannot decide twice.',
        ),
      },
    ];
  return [
    { text: NOT[kind] },
    {
      text:
        DECISION[code] ??
        msg(
          'Something unexpected happened. Nothing is assumed to have changed.',
        ),
    },
  ];
}

export function approvalsReadFailure(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? msg('Approvals could not be read: the authority did not answer.')
    : code === 'busy'
      ? msg(
          'Approvals could not be read. The authority is busy; try again in a moment.',
        )
      : msg(
          'Approvals could not be read. Nothing is shown rather than something unverified.',
        );
}

/** How much of a diff is shown, when it is not all of it. */
export function shownOf(
  text: string,
  bytes: number,
): { shown: string; total: string } {
  return {
    shown: new TextEncoder().encode(text).length.toLocaleString(),
    total: bytes.toLocaleString(),
  };
}
