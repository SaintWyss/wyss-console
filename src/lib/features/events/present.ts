import type {
  Disposition,
  EventCategory,
  EventRow,
  LEASE_SCOPES,
  RUNTIME_EXITS,
} from '../../contracts/events';
import type { SessionFailureCode } from '../../contracts/sessions';
import type { TimeFormat } from '../../contracts/settings';
import type { RuntimeTone } from '../../shell/runtime';
import { TURN_REASON } from '../agent/present';
import { APPROVAL_REASON } from '../approvals/present';
import type { EventFilter, EventScope, EventsStart } from './controller';

/** Presentation only: nothing here is read back as state. */

export const FILTER_LABEL: Record<EventFilter, string> = {
  all: 'All',
  sessions: 'Sessions',
  runtime: 'Runtime',
  turns: 'Turns',
  approvals: 'Approvals',
  security: 'Security',
};
const FILTER_CATEGORY: Partial<Record<EventFilter, EventCategory>> = {
  sessions: 'session',
  runtime: 'runtime',
  turns: 'turn',
  approvals: 'approval',
};

export const CATEGORY_LABEL: Record<EventCategory, string> = {
  session: 'Session',
  runtime: 'Runtime',
  turn: 'Turn',
  approval: 'Approval',
  lease: 'Lease',
  authority: 'Authority',
};

/**
 * How an event is marked. From its recorded kind and reason only: a mark says how the
 * thing ended, not how serious it is, and there is no alarm level here.
 */
export const DISPOSITION: Record<
  Disposition,
  { tone: RuntimeTone; label: string }
> = {
  normal: { tone: 'idle', label: '' },
  attention: { tone: 'warn', label: 'Needs a look' },
  failure: { tone: 'error', label: 'Failed' },
  security: { tone: 'error', label: 'Isolation' },
};

/** The loaded events a filter and a scope leave. Other events are not read for it. */
export function shownEvents(
  events: readonly EventRow[],
  filter: EventFilter,
  scope: EventScope | null,
): EventRow[] {
  const category = FILTER_CATEGORY[filter];
  return events.filter(
    (event) =>
      (filter === 'all' ||
        (filter === 'security'
          ? event.security
          : event.category === category)) &&
      (scope === null ||
        (event.session === scope.session &&
          (scope.turn === null || event.turn === scope.turn))),
  );
}

const SCOPE: Record<(typeof LEASE_SCOPES)[number], string> = {
  session_create: 'bind a workspace',
  session_resume: 'attach to Codex',
  turn_start: 'start a turn',
  command_execute: 'run approved commands',
  file_change: 'apply approved file changes',
};
const EXIT: Record<(typeof RUNTIME_EXITS)[number], string> = {
  unavailable: 'Codex was not available to start.',
  spawn_failed: 'The authority could not launch Codex.',
  transport_failed: 'The connection to Codex failed.',
  handshake_timeout: 'Codex did not answer in time when it started.',
  protocol_failed: 'Codex did not speak the protocol the authority expects.',
  server_request:
    'Codex asked for something this runtime is not allowed to answer.',
  runtime_rejected: 'Codex refused the configuration it was started with.',
  catalog_rejected: 'Codex refused the model catalog it was started with.',
  child_exited: 'The Codex process ended.',
  audit_unavailable: 'The authority could not record what Codex was doing.',
  inference_isolation_violation:
    'Codex acted outside what its isolation allows, and the authority stopped it.',
  safety_unproven:
    'The authority could not prove the isolation of the runtime, and did not let it run.',
};

/** What was recorded, in one line. Only what the record itself says. */
export function eventTitle(event: EventRow): string {
  const { facts } = event;
  switch (event.kind) {
    case 'session_prepared':
      return 'Managed session prepared';
    case 'session_closed':
      return 'Managed session closed';
    case 'session_bound':
      return 'Session bound to a workspace';
    case 'session_attached':
      return 'Session attached to Codex';
    case 'binding_indeterminate':
      return 'Workspace binding outcome unknown';
    case 'binding_abandoned':
      return 'Uncertain binding attempt discarded';
    case 'runtime_desired':
      return facts.desired === 'running'
        ? 'Codex runtime start accepted'
        : facts.desired === 'stopped'
          ? 'Codex runtime stop accepted'
          : 'Codex runtime request accepted';
    case 'runtime_ready':
      return 'Codex runtime ready';
    case 'runtime_exited':
      if (event.disposition === 'security') return 'Codex runtime contained';
      return facts.expected === true
        ? 'Codex runtime stopped'
        : 'Codex runtime ended unexpectedly';
    case 'turn_admitted':
      return 'Managed turn admitted';
    case 'turn_running':
      return 'Managed turn running';
    case 'turn_cancel_requested':
      return 'Turn cancellation requested';
    case 'turn_completed':
      return 'Managed turn completed';
    case 'turn_interrupted':
      return 'Managed turn interrupted';
    case 'turn_failed':
      return 'Managed turn failed';
    case 'turn_indeterminate':
      return 'Managed turn outcome unknown';
    case 'approval_requested':
      return facts.approval_kind === 'command_execution'
        ? 'Command approval requested'
        : facts.approval_kind === 'file_change'
          ? 'File change approval requested'
          : 'Approval requested';
    case 'approval_accepted':
      return 'Approval accepted';
    case 'approval_confirmed':
      return 'Approved action confirmed by Codex';
    case 'approval_declined':
      return 'Approval denied';
    case 'approval_cancelled':
      return 'Approval cancelled';
    case 'approval_expired':
      return 'Approval expired undecided';
    case 'approval_invalidated':
      return 'Approval no longer applies';
    case 'approval_indeterminate':
      return 'Approval outcome unknown';
    case 'lease_granted':
      return 'Execution lease granted';
    case 'lease_renewed':
      return 'Execution lease renewed';
    case 'lease_revoked':
      return 'Execution lease revoked';
    case 'lease_expired':
      return 'Execution lease expired';
    case 'other':
      return 'Authority event';
  }
}

/**
 * What the record says beyond its title, in a sentence or two. A reason is the
 * authority's own; nothing is inferred about what led to what.
 */
export function eventDetail(event: EventRow): string {
  const { facts } = event;
  switch (event.category) {
    case 'turn': {
      if (event.kind === 'turn_admitted')
        return 'The authority admitted the turn under an execution lease. What was asked of Codex is not part of the record.';
      if (event.kind === 'turn_running')
        return 'Codex took the turn and started working on it.';
      const reason = facts.turn_reason ? TURN_REASON[facts.turn_reason] : '';
      if (event.kind === 'turn_cancel_requested')
        return reason || 'A cancel of the turn was recorded.';
      return reason || 'The authority recorded how the turn ended.';
    }
    case 'approval': {
      const about =
        facts.approval_kind === 'command_execution'
          ? 'a command'
          : facts.approval_kind === 'file_change'
            ? 'a file change'
            : 'an action';
      if (event.kind === 'approval_requested')
        return `Codex proposed ${about} and the authority held it for a decision. The proposal itself is not part of the record.`;
      if (event.kind === 'approval_accepted')
        return `The approval of ${about} was accepted and the answer was sent to Codex.`;
      if (event.kind === 'approval_confirmed')
        return `Codex reported what it did with the approved ${about.replace(/^an? /, '')}.`;
      if (event.kind === 'approval_declined')
        return `The approval of ${about} was denied. Nothing was run or changed for it.`;
      const reason = facts.approval_reason
        ? APPROVAL_REASON[facts.approval_reason]
        : '';
      return reason || 'The authority recorded how the approval ended.';
    }
    case 'lease': {
      const scopes =
        facts.scopes && facts.scopes.length > 0
          ? facts.scopes.map((scope) => SCOPE[scope]).join(', ')
          : null;
      const allowed = scopes ? ` It allowed the session to ${scopes}.` : '';
      if (event.kind === 'lease_revoked')
        return `${facts.revocation === 'session_closed' ? 'The lease ended because its session was closed.' : 'The lease was given back.'}${allowed}`;
      if (event.kind === 'lease_expired')
        return `The lease reached its time limit.${allowed}`;
      return `The authority ${event.kind === 'lease_renewed' ? 'extended' : 'granted'} a lease for this session.${allowed}`;
    }
    case 'runtime':
      if (event.kind === 'runtime_desired')
        return 'The authority accepted a request for the managed Codex runtime and recorded what it should be.';
      if (event.kind === 'runtime_ready')
        return facts.runtime_version
          ? `Codex ${facts.runtime_version} started, was checked and is ready.`
          : 'Codex started, was checked and is ready.';
      return (
        (facts.exit ? EXIT[facts.exit] : 'The Codex process ended.') +
        (facts.expected === true ? ' The authority meant it to.' : '')
      );
    case 'session':
      switch (event.kind) {
        case 'session_prepared':
          return 'A managed session was created. It has no workspace until it is bound.';
        case 'session_closed':
          return 'The session was closed. Closing is final.';
        case 'session_bound':
          return 'The authority bound the session to a project folder. Which folder is not part of the record.';
        case 'session_attached':
          return 'The session was attached to the Codex runtime that was running.';
        case 'binding_indeterminate':
          return 'The authority could not confirm whether Codex created the workspace binding.';
        default:
          return 'An uncertain binding attempt was given up on explicitly.';
      }
    case 'authority':
      return 'The authority recorded an event of a kind this version of the console does not know. Nothing else of it is shown.';
  }
}

/** What lies before the oldest event shown. */
export function startMessage(start: EventsStart): string {
  return start === 'beginning'
    ? 'This is the beginning of the event stream.'
    : start === 'unavailable'
      ? 'History before this point is unavailable.'
      : '';
}

export function eventsReadFailure(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? 'Events could not be read: the authority did not answer.'
    : code === 'busy'
      ? 'Events could not be read. The authority is busy; try again in a moment.'
      : code === 'changed'
        ? 'The event history changed while it was being read.'
        : 'Events could not be read. Nothing is shown rather than something unverified.';
}
export function olderFailure(code: SessionFailureCode): string {
  return code === 'changed'
    ? 'Older events could not be read: the authority holds another history than the one shown. The newest events were read again.'
    : code === 'unavailable'
      ? 'Older events could not be read: the authority did not answer.'
      : 'Older events could not be read.';
}
const CLOCK_OPTIONS: Intl.DateTimeFormatOptions = {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
};
/** One formatter per preference; the system's own is the default. */
const CLOCKS: Record<TimeFormat, Intl.DateTimeFormat> = {
  system: new Intl.DateTimeFormat(undefined, CLOCK_OPTIONS),
  h12: new Intl.DateTimeFormat(undefined, { ...CLOCK_OPTIONS, hour12: true }),
  h24: new Intl.DateTimeFormat(undefined, {
    ...CLOCK_OPTIONS,
    hour12: false,
    hourCycle: 'h23',
  }),
};
const DAY = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
});
/** Local time of day of an event, to the second: several are recorded in one minute. */
export function eventClock(
  timestamp: string,
  format: TimeFormat = 'system',
): string {
  const time = Date.parse(timestamp);
  return Number.isNaN(time) ? timestamp : CLOCKS[format].format(time);
}
/** The local day of an event, when it is not the day it is read on. */
export function eventDay(timestamp: string, now: number = Date.now()): string {
  const time = Date.parse(timestamp);
  if (Number.isNaN(time)) return '';
  const [then, today] = [new Date(time), new Date(now)];
  const same =
    then.getFullYear() === today.getFullYear() &&
    then.getMonth() === today.getMonth() &&
    then.getDate() === today.getDate();
  if (same) return '';
  return then.getFullYear() === today.getFullYear()
    ? DAY.format(time)
    : `${DAY.format(time)}, ${then.getFullYear()}`;
}
/** A short form of an identity, for a line of text. The whole one is in the details. */
export function short(id: string): string {
  return id.slice(0, 8);
}
