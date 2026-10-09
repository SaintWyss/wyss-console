import type {
  SessionBinding,
  SessionFailureCode,
  SessionSummary,
} from '../../contracts/sessions';
import type { RuntimeTone } from '../../shell/runtime';
import type { ActivationStep, OperationKind } from './controller';

/** Presentation only: nothing here is read back as state. */

export function agentName(adapter: string): string {
  return adapter === 'codex' ? 'Codex' : adapter;
}

/** The last segment of the bound workspace, or a short form of the session's identity. */
export function sessionLabel(session: SessionSummary): string {
  const name = session.workspace?.split('/').filter(Boolean).at(-1);
  return name ?? `Session ${session.id.slice(0, 8)}`;
}

export const STATE: Record<
  SessionSummary['state'],
  { label: string; tone: RuntimeTone }
> = {
  prepared: { label: 'Prepared', tone: 'ok' },
  closed: { label: 'Closed', tone: 'idle' },
};

export const BINDING: Record<
  SessionBinding,
  { label: string; tone: RuntimeTone; detail: string }
> = {
  unbound: {
    label: 'No workspace',
    tone: 'idle',
    detail: 'No workspace is bound to this session.',
  },
  creating: {
    label: 'Binding',
    tone: 'pending',
    detail: 'The authority is binding a workspace to this session.',
  },
  indeterminate: {
    label: 'Uncertain',
    tone: 'warn',
    detail:
      'The authority could not confirm whether the workspace binding was created.',
  },
  bound_detached: {
    label: 'Detached',
    tone: 'idle',
    detail: 'Bound to a workspace and not attached to the running agent.',
  },
  bound_attached: {
    label: 'Ready',
    tone: 'ok',
    detail: 'Bound to a workspace and attached to the running agent.',
  },
};

const PENDING: Record<Exclude<OperationKind, 'activate'>, string> = {
  resume: 'Resuming session…',
  close: 'Closing session…',
  abandon: 'Discarding the uncertain attempt…',
};
const ACTIVATING: Record<ActivationStep, string> = {
  preparing: 'Preparing session…',
  starting: 'Starting Codex…',
  binding: 'Creating workspace binding…',
  attaching: 'Attaching…',
};
/** What a pending action is doing, in the words of its current step. */
export function pendingMessage(
  kind: OperationKind,
  step: ActivationStep | undefined,
): string {
  return kind === 'activate' ? ACTIVATING[step ?? 'starting'] : PENDING[kind];
}

const ACTION: Record<OperationKind, string> = {
  activate: 'The session is not ready',
  resume: 'The session was not resumed',
  close: 'The session was not closed',
  abandon: 'The attempt was not discarded',
};

const FAILURE: Record<SessionFailureCode, string> = {
  unavailable:
    'wyss-control did not answer, so the result is unknown. Retrying repeats the same request safely.',
  incompatible: 'This console and wyss-control speak different versions.',
  invalid: 'The console sent a request it should not have. Nothing changed.',
  not_found: 'The authority no longer has this session.',
  changed: 'The session changed after it was shown. It has been read again.',
  not_allowed: 'The session is closed, so the authority refused.',
  not_bound: 'This session has no workspace binding to resume.',
  runtime_unavailable:
    'The authority denied it because the agent runtime is not ready.',
  start_refused: 'wyss-control refused to start Codex.',
  workspace_invalid:
    'wyss-control did not accept that folder as a workspace. Nothing was bound.',
  already_bound:
    'The session already has a workspace binding. It has been read again.',
  permission_denied:
    'The authority denied it: no execution lease allows this for the session.',
  busy: 'The authority is busy with another operation. Try again in a moment.',
  rejected: 'The agent refused to resume this session.',
  uncertain:
    'The authority could not confirm what the agent did. Check the session before trying again.',
  conflict: 'That request was already used for something else. Try again.',
  internal:
    'Something unexpected happened. Nothing is assumed to have changed.',
};
/** Where an action gives a code another meaning than the common one. */
const SPECIFIC: Partial<
  Record<OperationKind, Partial<Record<SessionFailureCode, string>>>
> = {
  activate: {
    not_allowed: 'The authority refused: the session no longer allows it.',
    runtime_unavailable:
      'Codex is not ready, so the session could not be bound or attached.',
    rejected: 'Codex refused the request.',
  },
  abandon: {
    not_allowed:
      'The authority holds no uncertain attempt for this session any more.',
  },
};

/**
 * One sentence for a failed action. An unknown outcome is never worded as a refusal:
 * neither an answer that did not come nor an activation the authority could not confirm.
 */
export function failureMessage(
  kind: OperationKind,
  code: SessionFailureCode,
): string {
  if (code === 'unavailable') return FAILURE[code];
  if (kind === 'activate' && code === 'uncertain')
    return 'Activation outcome uncertain: wyss-control could not confirm whether Codex created the workspace binding. Nothing is sent again by itself.';
  return `${ACTION[kind]}. ${SPECIFIC[kind]?.[code] ?? FAILURE[code]}`;
}

/**
 * Whether a failed action is one whose outcome is not known, as opposed to one that
 * was refused. It is shown as a caution, not as an error.
 */
export function outcomeUnknown(code: SessionFailureCode): boolean {
  return code === 'unavailable' || code === 'uncertain';
}

export function readFailureMessage(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? 'Sessions could not be read: wyss-control did not answer.'
    : code === 'incompatible'
      ? `Sessions could not be read. ${FAILURE.incompatible}`
      : code === 'busy'
        ? `Sessions could not be read. ${FAILURE.busy}`
        : 'Sessions could not be read. Nothing is shown rather than something unverified.';
}

/** Why no folder came back from the dialog, when that is not the user's own doing. */
export function chooseFailureMessage(code: SessionFailureCode): string {
  return code === 'workspace_invalid'
    ? 'That folder cannot be used: its path is too long or cannot be written as text.'
    : code === 'busy'
      ? 'The folder dialog is already open.'
      : 'The folder dialog could not be opened.';
}

/** Why Resume is not offered, when it is not. Null when it is. */
export function resumeHint(
  session: SessionSummary,
  agentReady: boolean,
): string | null {
  if (session.state === 'closed') return 'A closed session cannot be resumed.';
  if (session.binding === 'bound_attached')
    return 'Ready: attached to the running agent.';
  // The binding itself is described next to it; this says what it means for Resume.
  if (!session.resumable)
    return session.binding === 'unbound'
      ? 'Resume needs a workspace bound to the session.'
      : 'Resume has to wait until the workspace binding is settled.';
  if (!agentReady)
    return 'The agent runtime is not ready, so there is nothing to attach to.';
  return null;
}

const DATE = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});
/** Local date and time of an authority timestamp. */
export function localTime(timestamp: string): string {
  const time = Date.parse(timestamp);
  return Number.isNaN(time) ? timestamp : DATE.format(time);
}
