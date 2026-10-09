import type {
  SessionBinding,
  SessionFailureCode,
  SessionSummary,
} from '../../contracts/sessions';
import type { RuntimeTone } from '../../shell/runtime';
import type { OperationKind } from './controller';

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
    detail: 'No workspace is bound to this session yet.',
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
    label: 'Attached',
    tone: 'ok',
    detail: 'Bound to a workspace and attached to the running agent.',
  },
};

export const PENDING: Record<OperationKind, string> = {
  prepare: 'Creating session…',
  resume: 'Resuming session…',
  close: 'Closing session…',
};

const ACTION: Record<OperationKind, string> = {
  prepare: 'The session was not created',
  resume: 'The session was not resumed',
  close: 'The session was not closed',
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

/** One sentence for a failed action. An unknown outcome is never worded as a refusal. */
export function failureMessage(
  kind: OperationKind,
  code: SessionFailureCode,
): string {
  return code === 'unavailable'
    ? FAILURE[code]
    : `${ACTION[kind]}. ${FAILURE[code]}`;
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

/** Why Resume is not offered, when it is not. Null when it is. */
export function resumeHint(
  session: SessionSummary,
  agentReady: boolean,
): string | null {
  if (session.state === 'closed') return 'A closed session cannot be resumed.';
  if (session.binding === 'bound_attached')
    return 'Already attached to the running agent.';
  if (!session.resumable) return BINDING[session.binding].detail;
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
