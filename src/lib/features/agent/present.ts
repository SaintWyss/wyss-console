import type { SessionFailureCode, SessionMode } from '../../contracts/sessions';
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
  // Both a cancelled turn and a denied command end this way at the authority.
  user_cancel: 'Stopped at your request.',
  lease_expired: 'Its execution lease expired, so the authority stopped it.',
  lease_revoked:
    'Its execution lease was revoked, so the authority stopped it.',
  session_closed: 'The session was closed while it ran.',
  runtime_stopped: 'Codex was stopped while it ran.',
  authority_shutdown: 'The authority shut down while it ran.',
  authority_restarted:
    'The authority restarted while it ran. What Codex did after that is not known.',
  runtime_failed: 'Codex stopped unexpectedly.',
  provider_failed: 'Codex reported that the turn failed.',
  dispatch_unknown:
    'The authority could not confirm whether Codex received the turn.',
  inference_isolation_violation:
    'Codex tried to act outside what this session allows, and the authority stopped it.',
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
/**
 * What the turn waits on, while what the approval is about has not been read yet. It
 * names the kind of action, never the action.
 */
export function approvalMessage(kind: ApprovalKind, until: string): string {
  return `Codex asked to ${ASKED[kind]}. The authority holds the turn until that is decided, or until ${until}.`;
}

const MODE: Record<SessionMode, string> = {
  inference: 'Codex · Inference',
  local_tools: 'Codex · Local Tools',
  unknown: 'Codex',
};
/** Who answers in this session, and in which mode it is attached. */
export function agentLabel(mode: SessionMode | undefined): string {
  return MODE[mode ?? 'unknown'];
}

const COMMON: Record<SessionFailureCode, string> = {
  unavailable: 'The authority did not answer.',
  incompatible: 'This console and the authority speak different versions.',
  invalid: 'The console sent a request it should not have. Nothing changed.',
  not_found: 'The authority no longer has it.',
  changed: 'It changed after it was shown. It has been read again.',
  not_allowed: 'The authority refused: it is no longer allowed.',
  not_bound: 'The session has no workspace. Activate it in Sessions.',
  runtime_unavailable:
    'The session is not attached to a running Codex. Resume it in Sessions.',
  start_refused: 'The authority refused to start Codex.',
  turn_active: 'A turn is running. The runtime is not restarted under it.',
  mode_fixed:
    'The session is attached in the other mode: its conversation exists only there.',
  workspace_invalid: 'The authority did not accept the workspace.',
  already_bound: 'The session already has a workspace binding.',
  permission_denied:
    'The authority denied it: no execution lease allows a turn for this session.',
  busy: 'The authority is busy with another operation. Try again in a moment.',
  rejected: 'Codex refused it.',
  uncertain:
    'The authority could not confirm what Codex did. Nothing is sent again by itself.',
  conflict: 'That request was already used for something else.',
  internal:
    'Something unexpected happened. Nothing is assumed to have changed.',
};

const START: Partial<Record<SessionFailureCode, string>> = {
  unavailable:
    'The authority did not answer, so it is not known whether the turn started. Retrying repeats the same request: it cannot start a second turn.',
  invalid: 'The input is empty or longer than a turn can carry.',
  not_found: 'The authority no longer has this session.',
  changed: 'The session changed after it was shown. It has been read again.',
  not_allowed: 'The session is closed, so the authority refused.',
  busy: 'Another turn is running. The authority runs one turn at a time.',
  rejected: 'Codex refused the turn.',
  uncertain:
    'The authority could not confirm whether Codex received the turn. Nothing is sent again by itself.',
  conflict: 'That request was already used for another input.',
};
/** One sentence for a start that failed. An unknown outcome is never worded as a refusal. */
export function startFailureMessage(code: SessionFailureCode): string {
  if (code === 'unavailable' || code === 'uncertain') return START[code]!;
  return `The turn was not started. ${START[code] ?? COMMON[code]}`;
}

const CANCEL: Partial<Record<SessionFailureCode, string>> = {
  unavailable:
    'The authority did not answer, so it is not known whether the cancel was taken. The turn is shown as the authority reports it.',
  not_found: 'The authority no longer has this turn.',
  changed: 'The turn changed after it was shown. Try again.',
  not_allowed: 'The turn had already ended.',
};
export function cancelFailureMessage(code: SessionFailureCode): string {
  if (code === 'unavailable') return CANCEL[code]!;
  return `The cancel was not taken. ${CANCEL[code] ?? COMMON[code]}`;
}

export function readFailureMessage(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? 'This could not be read: the authority did not answer.'
    : code === 'not_found'
      ? 'This could not be read: the authority no longer has it.'
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
      ? 'Connecting to the authority…'
      : 'The authority is unavailable: a turn cannot be started.';
  if (view.start?.status === 'pending') return 'Starting the turn…';
  if (view.start) return 'Retry or dismiss the turn that was not started.';
  const facts = view.facts;
  if (facts === null)
    return view.readFailure
      ? 'The session could not be read.'
      : 'Reading the session…';
  if (facts.state === 'closed') return 'This session is closed.';
  if (view.active !== null)
    return 'A turn is running. The authority runs one at a time.';
  if (facts.binding === 'unbound')
    return 'This session has no workspace. Activate it in Sessions.';
  if (facts.binding === 'bound_detached')
    return 'This session is not attached to Codex. Resume it in Sessions.';
  if (facts.binding !== 'bound_attached')
    return 'The workspace binding of this session is not settled.';
  if (!view.runnable)
    return 'The authority reports that a turn cannot start now.';
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
