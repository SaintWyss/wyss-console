import type { SessionFailureCode } from '../../contracts/sessions';
import {
  PROMPT_MAX_BYTES,
  type ApprovalKind,
  type ApprovalOutcome,
  type TurnEventKind,
  type TurnReason,
  type TurnState,
} from '../../contracts/turns';
import type { RuntimeTone } from '../../shell/runtime';
import type { AgentLink, AgentSessionView } from './controller';

/** Presentation only: nothing here is read back as state. */

/** The authority's lifecycle names, as they are. */
export const TURN_STATE: Record<
  TurnState,
  { label: string; tone: RuntimeTone }
> = {
  starting: { label: 'Starting', tone: 'pending' },
  running: { label: 'Running', tone: 'ok' },
  completed: { label: 'Completed', tone: 'idle' },
  interrupted: { label: 'Interrupted', tone: 'warn' },
  failed: { label: 'Failed', tone: 'error' },
  indeterminate: { label: 'Indeterminate', tone: 'warn' },
};

/** Why a turn ended, in one sentence. Empty when the state already says it. */
export const TURN_REASON: Record<TurnReason, string> = {
  completed: '',
  user_cancel: 'Cancelled at your request.',
  lease_expired: 'Its execution lease expired, so wyss-control stopped it.',
  lease_revoked: 'Its execution lease was revoked, so wyss-control stopped it.',
  session_closed: 'The session was closed while it ran.',
  runtime_stopped: 'Codex was stopped while it ran.',
  authority_shutdown: 'wyss-control shut down while it ran.',
  authority_restarted:
    'wyss-control restarted while it ran. What Codex did after that is not known.',
  runtime_failed: 'Codex stopped unexpectedly.',
  provider_failed: 'Codex reported that the turn failed.',
  dispatch_unknown:
    'wyss-control could not confirm whether Codex received the turn.',
  inference_isolation_violation:
    'Codex tried to act outside what this session allows, and wyss-control stopped it.',
  interrupt_timeout:
    'Codex did not confirm the cancel in time. What it did is not known.',
  approval_expired: 'The approval it waited on expired undecided.',
  action_rejected: 'Codex asked for an action this session cannot allow.',
};

const ACTIVITY: Partial<Record<TurnEventKind, string>> = {
  command_proposed: 'Command proposed',
  command_started: 'Command started',
  command_output: 'Command output',
  command_completed: 'Command finished',
  command_failed: 'Command failed',
  file_change_proposed: 'File change proposed',
  file_change_started: 'File change started',
  file_change_completed: 'File change applied',
  file_change_failed: 'File change failed',
  approval_required: 'Approval requested',
};
/** What a mediated action did. Never its content: the window is not given it. */
export function activityLabel(kind: TurnEventKind, count: number): string {
  const label = ACTIVITY[kind] ?? 'Activity';
  return count > 1 ? `${label} · ${count}` : label;
}

const OUTCOME: Record<ApprovalOutcome, string> = {
  accepted: 'Approval accepted',
  declined: 'Approval declined',
  cancelled: 'Approval cancelled',
  expired: 'Approval expired undecided',
  invalidated: 'Approval no longer applies',
  indeterminate: 'Approval outcome unknown',
};
export function outcomeLabel(outcome: ApprovalOutcome | null): string {
  return outcome ? OUTCOME[outcome] : 'Approval closed';
}

const ASKED: Record<ApprovalKind, string> = {
  command_execution: 'run a command',
  file_change: 'change files',
};
/** What the turn waits on. It names the kind of action, never the action. */
export function approvalMessage(kind: ApprovalKind, until: string): string {
  return `Codex asked to ${ASKED[kind]}. wyss-control holds the turn until that is decided, or until ${until}. This view cannot answer it.`;
}

const COMMON: Record<SessionFailureCode, string> = {
  unavailable: 'wyss-control did not answer.',
  incompatible: 'This console and wyss-control speak different versions.',
  invalid: 'The console sent a request it should not have. Nothing changed.',
  not_found: 'wyss-control no longer has it.',
  changed: 'It changed after it was shown. It has been read again.',
  not_allowed: 'wyss-control refused: it is no longer allowed.',
  not_bound: 'The session has no workspace. Activate it in Sessions.',
  runtime_unavailable:
    'The session is not attached to a running Codex. Resume it in Sessions.',
  start_refused: 'wyss-control refused to start Codex.',
  workspace_invalid: 'wyss-control did not accept the workspace.',
  already_bound: 'The session already has a workspace binding.',
  permission_denied:
    'wyss-control denied it: no execution lease allows a turn for this session.',
  busy: 'wyss-control is busy with another operation. Try again in a moment.',
  rejected: 'Codex refused it.',
  uncertain:
    'wyss-control could not confirm what Codex did. Nothing is sent again by itself.',
  conflict: 'That request was already used for something else.',
  internal:
    'Something unexpected happened. Nothing is assumed to have changed.',
};

const START: Partial<Record<SessionFailureCode, string>> = {
  unavailable:
    'wyss-control did not answer, so it is not known whether the turn started. Retrying repeats the same request: it cannot start a second turn.',
  invalid: 'The input is empty or longer than a turn can carry.',
  not_found: 'wyss-control no longer has this session.',
  changed: 'The session changed after it was shown. It has been read again.',
  not_allowed: 'The session is closed, so wyss-control refused.',
  busy: 'Another turn is running. wyss-control runs one turn at a time.',
  rejected: 'Codex refused the turn.',
  uncertain:
    'wyss-control could not confirm whether Codex received the turn. Nothing is sent again by itself.',
  conflict: 'That request was already used for another input.',
};
/** One sentence for a start that failed. An unknown outcome is never worded as a refusal. */
export function startFailureMessage(code: SessionFailureCode): string {
  if (code === 'unavailable' || code === 'uncertain') return START[code]!;
  return `The turn was not started. ${START[code] ?? COMMON[code]}`;
}

const CANCEL: Partial<Record<SessionFailureCode, string>> = {
  unavailable:
    'wyss-control did not answer, so it is not known whether the cancel was taken. The turn is shown as wyss-control reports it.',
  not_found: 'wyss-control no longer has this turn.',
  changed: 'The turn changed after it was shown. Try again.',
  not_allowed: 'The turn had already ended.',
};
export function cancelFailureMessage(code: SessionFailureCode): string {
  if (code === 'unavailable') return CANCEL[code]!;
  return `The cancel was not taken. ${CANCEL[code] ?? COMMON[code]}`;
}

export function readFailureMessage(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? 'This could not be read: wyss-control did not answer.'
    : code === 'not_found'
      ? 'This could not be read: wyss-control no longer has it.'
      : code === 'busy'
        ? `This could not be read. ${COMMON.busy}`
        : 'This could not be read. Nothing is shown rather than something unverified.';
}

/** Whether a failure is an outcome that is not known, as opposed to a refusal. */
export function outcomeUnknown(code: SessionFailureCode): boolean {
  return code === 'unavailable' || code === 'uncertain';
}

export function overLimit(draft: string): boolean {
  return new TextEncoder().encode(draft).length > PROMPT_MAX_BYTES;
}

/**
 * Why Run is not offered, from what was read. Null when it is. It words a hint: every
 * start that is sent is decided by the authority.
 */
export function runHint(
  link: AgentLink,
  view: AgentSessionView,
): string | null {
  if (link !== 'connected')
    return link === 'connecting'
      ? 'Connecting to wyss-control…'
      : 'wyss-control is unavailable: a turn cannot be started.';
  if (view.start?.status === 'pending') return 'Starting the turn…';
  if (view.start) return 'Retry or dismiss the turn that was not started.';
  const facts = view.facts;
  if (facts === null)
    return view.readFailure
      ? 'The session could not be read.'
      : 'Reading the session…';
  if (facts.state === 'closed') return 'This session is closed.';
  if (view.active !== null)
    return 'A turn is running. wyss-control runs one at a time.';
  if (facts.binding === 'unbound')
    return 'This session has no workspace. Activate it in Sessions.';
  if (facts.binding === 'bound_detached')
    return 'This session is not attached to Codex. Resume it in Sessions.';
  if (facts.binding !== 'bound_attached')
    return 'The workspace binding of this session is not settled.';
  if (!view.runnable)
    return 'wyss-control reports that a turn cannot start now.';
  if (overLimit(view.draft))
    return 'The input is longer than one turn can carry.';
  return null;
}

const TIME = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });
/** Local time of day of an authority timestamp. */
export function localClock(timestamp: string): string {
  const time = Date.parse(timestamp);
  return Number.isNaN(time) ? timestamp : TIME.format(time);
}
