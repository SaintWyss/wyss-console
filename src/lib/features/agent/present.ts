import type { SessionFailureCode, SessionMode } from '../../contracts/sessions';
import {
  PROMPT_MAX_BYTES,
  type ApprovalKind,
  type ApprovalOutcome,
  type TurnEventKind,
  type TurnReason,
  type TurnState,
} from '../../contracts/turns';
import { msg, type Phrase } from '../../i18n/msg';
import type { RuntimeTone } from '../../shell/runtime';
import type { AgentLink, AgentSessionView } from './controller';

/** Presentation only: nothing here is read back as state. */

/** The authority's lifecycle names, as they are. */
export const TURN_STATE: Record<
  TurnState,
  { label: string; tone: RuntimeTone }
> = {
  starting: { label: msg('Starting'), tone: 'pending' },
  running: { label: msg('Running'), tone: 'ok' },
  completed: { label: msg('Completed'), tone: 'idle' },
  interrupted: { label: msg('Interrupted'), tone: 'warn' },
  failed: { label: msg('Failed'), tone: 'error' },
  indeterminate: { label: msg('Indeterminate'), tone: 'warn' },
};

/** Why a turn ended, in one sentence. Empty when the state already says it. */
export const TURN_REASON: Record<TurnReason, string> = {
  completed: '',
  // Both a cancelled turn and a denied command end this way at the authority.
  user_cancel: msg('Stopped at your request.'),
  lease_expired: msg(
    'Its execution lease expired, so the authority stopped it.',
  ),
  lease_revoked: msg(
    'Its execution lease was revoked, so the authority stopped it.',
  ),
  session_closed: msg('The session was closed while it ran.'),
  runtime_stopped: msg('Codex was stopped while it ran.'),
  authority_shutdown: msg('The authority shut down while it ran.'),
  authority_restarted: msg(
    'The authority restarted while it ran. What Codex did after that is not known.',
  ),
  runtime_failed: msg('Codex stopped unexpectedly.'),
  provider_failed: msg('Codex reported that the turn failed.'),
  dispatch_unknown: msg(
    'The authority could not confirm whether Codex received the turn.',
  ),
  inference_isolation_violation: msg(
    'Codex tried to act outside what this session allows, and the authority stopped it.',
  ),
  interrupt_timeout: msg(
    'Codex did not confirm the cancel in time. What it did is not known.',
  ),
  approval_expired: msg('The approval it waited on expired undecided.'),
  action_rejected: msg('Codex asked for an action this session cannot allow.'),
};

const ACTIVITY: Partial<Record<TurnEventKind, string>> = {
  command_proposed: msg('Command proposed'),
  command_started: msg('Command started'),
  command_output: msg('Command output'),
  command_completed: msg('Command finished'),
  command_failed: msg('Command failed'),
  file_change_proposed: msg('File change proposed'),
  file_change_started: msg('File change started'),
  file_change_completed: msg('File change applied'),
  file_change_failed: msg('File change failed'),
  approval_required: msg('Approval requested'),
};
/**
 * What a mediated action did, and how many times it was seen. Never its content: the window
 * is not given it. The count, when there is one, is joined after the label.
 */
export function activityLabel(kind: TurnEventKind, count: number): Phrase[] {
  const label: Phrase = { text: ACTIVITY[kind] ?? msg('Activity') };
  return count > 1 ? [label, { text: String(count) }] : [label];
}

const OUTCOME: Record<ApprovalOutcome, string> = {
  accepted: msg('Approval accepted'),
  declined: msg('Approval declined'),
  cancelled: msg('Approval cancelled'),
  expired: msg('Approval expired undecided'),
  invalidated: msg('Approval no longer applies'),
  indeterminate: msg('Approval outcome unknown'),
};
export function outcomeLabel(outcome: ApprovalOutcome | null): string {
  return outcome ? OUTCOME[outcome] : msg('Approval closed');
}

/**
 * What the turn waits on, while what the approval is about has not been read yet. It
 * names the kind of action, never the action.
 */
const ASKED: Record<ApprovalKind, string> = {
  command_execution: msg(
    'Codex asked to run a command. The authority holds the turn until that is decided, or until {until}.',
  ),
  file_change: msg(
    'Codex asked to change files. The authority holds the turn until that is decided, or until {until}.',
  ),
};
export function approvalMessage(kind: ApprovalKind, until: string): Phrase {
  return { text: ASKED[kind], values: { until } };
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
  unavailable: msg('The authority did not answer.'),
  incompatible: msg('This console and the authority speak different versions.'),
  invalid: msg(
    'The console sent a request it should not have. Nothing changed.',
  ),
  not_found: msg('The authority no longer has it.'),
  changed: msg('It changed after it was shown. It has been read again.'),
  not_allowed: msg('The authority refused: it is no longer allowed.'),
  not_bound: msg('The session has no workspace. Activate it in Sessions.'),
  runtime_unavailable: msg(
    'The session is not attached to a running Codex. Resume it in Sessions.',
  ),
  start_refused: msg('The authority refused to start Codex.'),
  turn_active: msg('A turn is running. The runtime is not restarted under it.'),
  mode_fixed: msg(
    'The session is attached in the other mode: its conversation exists only there.',
  ),
  workspace_invalid: msg('The authority did not accept the workspace.'),
  already_bound: msg('The session already has a workspace binding.'),
  permission_denied: msg(
    'The authority denied it: no execution lease allows a turn for this session.',
  ),
  busy: msg(
    'The authority is busy with another operation. Try again in a moment.',
  ),
  rejected: msg('Codex refused it.'),
  uncertain: msg(
    'The authority could not confirm what Codex did. Nothing is sent again by itself.',
  ),
  conflict: msg('That request was already used for something else.'),
  internal: msg(
    'Something unexpected happened. Nothing is assumed to have changed.',
  ),
};

const START: Partial<Record<SessionFailureCode, string>> = {
  unavailable: msg(
    'The authority did not answer, so it is not known whether the turn started. Retrying repeats the same request: it cannot start a second turn.',
  ),
  invalid: msg('The input is empty or longer than a turn can carry.'),
  not_found: msg('The authority no longer has this session.'),
  changed: msg(
    'The session changed after it was shown. It has been read again.',
  ),
  not_allowed: msg('The session is closed, so the authority refused.'),
  busy: msg('Another turn is running. The authority runs one turn at a time.'),
  rejected: msg('Codex refused the turn.'),
  uncertain: msg(
    'The authority could not confirm whether Codex received the turn. Nothing is sent again by itself.',
  ),
  conflict: msg('That request was already used for another input.'),
};
/** One sentence for a start that failed. An unknown outcome is never worded as a refusal. */
export function startFailureMessage(code: SessionFailureCode): Phrase {
  if (code === 'unavailable' || code === 'uncertain')
    return { text: START[code]! };
  return {
    text: msg('The turn was not started. {reason}'),
    values: { reason: { text: START[code] ?? COMMON[code] } },
  };
}

const CANCEL: Partial<Record<SessionFailureCode, string>> = {
  unavailable: msg(
    'The authority did not answer, so it is not known whether the cancel was taken. The turn is shown as the authority reports it.',
  ),
  not_found: msg('The authority no longer has this turn.'),
  changed: msg('The turn changed after it was shown. Try again.'),
  not_allowed: msg('The turn had already ended.'),
};
export function cancelFailureMessage(code: SessionFailureCode): Phrase {
  if (code === 'unavailable') return { text: CANCEL[code]! };
  return {
    text: msg('The cancel was not taken. {reason}'),
    values: { reason: { text: CANCEL[code] ?? COMMON[code] } },
  };
}

export function readFailureMessage(code: SessionFailureCode): Phrase {
  if (code === 'unavailable')
    return {
      text: msg('This could not be read: the authority did not answer.'),
    };
  if (code === 'not_found')
    return {
      text: msg('This could not be read: the authority no longer has it.'),
    };
  if (code === 'busy')
    return {
      text: msg('This could not be read. {reason}'),
      values: { reason: { text: COMMON.busy } },
    };
  return {
    text: msg(
      'This could not be read. Nothing is shown rather than something unverified.',
    ),
  };
}

/** One sentence for older turns that could not be loaded. Nothing is shown in their place. */
export function historyFailureMessage(code: SessionFailureCode): Phrase {
  switch (code) {
    case 'unavailable':
      return {
        text: msg(
          'Older turns could not be loaded: the authority did not answer.',
        ),
      };
    case 'changed':
      return {
        text: msg(
          'The history changed while it was being read, so the place reached in it is no longer good. Reload the turns to start again from the newest.',
        ),
      };
    case 'not_found':
      return {
        text: msg(
          'Older turns could not be loaded: the authority no longer has this session.',
        ),
      };
    case 'busy':
      return {
        text: msg('Older turns could not be loaded. {reason}'),
        values: { reason: { text: COMMON.busy } },
      };
    default:
      return {
        text: msg(
          'Older turns could not be loaded. Nothing is shown rather than something unverified.',
        ),
      };
  }
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
      ? msg('Connecting to the authority…')
      : msg('The authority is unavailable: a turn cannot be started.');
  if (view.start?.status === 'pending') return msg('Starting the turn…');
  if (view.start) return msg('Retry or dismiss the turn that was not started.');
  const facts = view.facts;
  if (facts === null)
    return view.readFailure
      ? msg('The session could not be read.')
      : msg('Reading the session…');
  if (facts.state === 'closed') return msg('This session is closed.');
  if (view.active !== null)
    return msg('A turn is running. The authority runs one at a time.');
  if (facts.binding === 'unbound')
    return msg('This session has no workspace. Activate it in Sessions.');
  if (facts.binding === 'bound_detached')
    return msg('This session is not attached to Codex. Resume it in Sessions.');
  if (facts.binding !== 'bound_attached')
    return msg('The workspace binding of this session is not settled.');
  if (!view.runnable)
    return msg('The authority reports that a turn cannot start now.');
  if (overLimit(view.draft))
    return msg('The input is longer than one turn can carry.');
  return null;
}

const TIME = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });
/** Local time of day of an authority timestamp. */
export function localClock(timestamp: string): string {
  const time = Date.parse(timestamp);
  return Number.isNaN(time) ? timestamp : TIME.format(time);
}
