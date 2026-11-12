import type {
  Disposition,
  EventCategory,
  EventRow,
  LEASE_SCOPES,
  RUNTIME_EXITS,
} from '../../contracts/events';
import type { SessionFailureCode } from '../../contracts/sessions';
import type { TimeFormat } from '../../contracts/settings';
import { msg, type Phrase } from '../../i18n/msg';
import type { RuntimeTone } from '../../shell/runtime';
import { TURN_REASON } from '../agent/present';
import { APPROVAL_REASON } from '../approvals/present';
import type { EventFilter, EventScope, EventsStart } from './controller';

/** Presentation only: nothing here is read back as state. */

export const FILTER_LABEL: Record<EventFilter, string> = {
  all: msg('All'),
  sessions: msg('Sessions'),
  runtime: msg('Runtime'),
  turns: msg('Turns'),
  approvals: msg('Approvals'),
  security: msg('Security'),
};
const FILTER_CATEGORY: Partial<Record<EventFilter, EventCategory>> = {
  sessions: 'session',
  runtime: 'runtime',
  turns: 'turn',
  approvals: 'approval',
};

export const CATEGORY_LABEL: Record<EventCategory, string> = {
  session: msg('Session'),
  runtime: msg('Runtime'),
  turn: msg('Turn'),
  approval: msg('Approval'),
  lease: msg('Lease'),
  authority: msg('Authority'),
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
  attention: { tone: 'warn', label: msg('Needs a look') },
  failure: { tone: 'error', label: msg('Failed') },
  security: { tone: 'error', label: msg('Isolation') },
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
  session_create: msg('bind a workspace'),
  session_resume: msg('attach to Codex'),
  turn_start: msg('start a turn'),
  command_execute: msg('run approved commands'),
  file_change: msg('apply approved file changes'),
};
const EXIT: Record<(typeof RUNTIME_EXITS)[number], string> = {
  unavailable: msg('Codex was not available to start.'),
  spawn_failed: msg('The authority could not launch Codex.'),
  transport_failed: msg('The connection to Codex failed.'),
  handshake_timeout: msg('Codex did not answer in time when it started.'),
  protocol_failed: msg(
    'Codex did not speak the protocol the authority expects.',
  ),
  server_request: msg(
    'Codex asked for something this runtime is not allowed to answer.',
  ),
  runtime_rejected: msg('Codex refused the configuration it was started with.'),
  catalog_rejected: msg('Codex refused the model catalog it was started with.'),
  child_exited: msg('The Codex process ended.'),
  audit_unavailable: msg(
    'The authority could not record what Codex was doing.',
  ),
  inference_isolation_violation: msg(
    'Codex acted outside what its isolation allows, and the authority stopped it.',
  ),
  safety_unproven: msg(
    'The authority could not prove the isolation of the runtime, and did not let it run.',
  ),
};

/** What was recorded, in one line. Only what the record itself says. */
export function eventTitle(event: EventRow): string {
  const { facts } = event;
  switch (event.kind) {
    case 'session_prepared':
      return msg('Managed session prepared');
    case 'session_closed':
      return msg('Managed session closed');
    case 'session_bound':
      return msg('Session bound to a workspace');
    case 'session_attached':
      return msg('Session attached to Codex');
    case 'binding_indeterminate':
      return msg('Workspace binding outcome unknown');
    case 'binding_abandoned':
      return msg('Uncertain binding attempt discarded');
    case 'runtime_desired':
      return facts.desired === 'running'
        ? msg('Codex runtime start accepted')
        : facts.desired === 'stopped'
          ? msg('Codex runtime stop accepted')
          : msg('Codex runtime request accepted');
    case 'runtime_ready':
      return msg('Codex runtime ready');
    case 'runtime_exited':
      if (event.disposition === 'security')
        return msg('Codex runtime contained');
      return facts.expected === true
        ? msg('Codex runtime stopped')
        : msg('Codex runtime ended unexpectedly');
    case 'turn_admitted':
      return msg('Managed turn admitted');
    case 'turn_running':
      return msg('Managed turn running');
    case 'turn_cancel_requested':
      return msg('Turn cancellation requested');
    case 'turn_completed':
      return msg('Managed turn completed');
    case 'turn_interrupted':
      return msg('Managed turn interrupted');
    case 'turn_failed':
      return msg('Managed turn failed');
    case 'turn_indeterminate':
      return msg('Managed turn outcome unknown');
    case 'approval_requested':
      return facts.approval_kind === 'command_execution'
        ? msg('Command approval requested')
        : facts.approval_kind === 'file_change'
          ? msg('File change approval requested')
          : msg('Approval requested');
    case 'approval_accepted':
      return msg('Approval accepted');
    case 'approval_confirmed':
      return msg('Approved action confirmed by Codex');
    case 'approval_declined':
      return msg('Approval denied');
    case 'approval_cancelled':
      return msg('Approval cancelled');
    case 'approval_expired':
      return msg('Approval expired undecided');
    case 'approval_invalidated':
      return msg('Approval no longer applies');
    case 'approval_indeterminate':
      return msg('Approval outcome unknown');
    case 'lease_granted':
      return msg('Execution lease granted');
    case 'lease_renewed':
      return msg('Execution lease renewed');
    case 'lease_revoked':
      return msg('Execution lease revoked');
    case 'lease_expired':
      return msg('Execution lease expired');
    case 'other':
      return msg('Authority event');
  }
}

/** Phrases drawn as one list joined with a comma. Each part is translated where it is drawn. */
function listOf(parts: readonly Phrase[]): Phrase {
  return {
    text: parts.map((_, index) => `{part${index}}`).join(', '),
    values: Object.fromEntries(
      parts.map((part, index) => [`part${index}`, part]),
    ),
  };
}

/**
 * What an approval is about, as the sentence needs it: the noun with its article, and the
 * same thing once it is approved. Spanish takes the article and the gender from the noun.
 */
function subjectOf(approvalKind: EventRow['facts']['approval_kind']): {
  about: Phrase;
  approved: Phrase;
} {
  if (approvalKind === 'command_execution')
    return {
      about: { text: msg('a command') },
      approved: { text: msg('the approved command') },
    };
  if (approvalKind === 'file_change')
    return {
      about: { text: msg('a file change') },
      approved: { text: msg('the approved file change') },
    };
  return {
    about: { text: msg('an action') },
    approved: { text: msg('the approved action') },
  };
}

/**
 * What the record says beyond its title, in a sentence or two. A reason is the
 * authority's own; nothing is inferred about what led to what.
 */
export function eventDetail(event: EventRow): Phrase {
  const { facts } = event;
  switch (event.category) {
    case 'turn': {
      if (event.kind === 'turn_admitted')
        return {
          text: msg(
            'The authority admitted the turn under an execution lease. What was asked of Codex is not part of the record.',
          ),
        };
      if (event.kind === 'turn_running')
        return { text: msg('Codex took the turn and started working on it.') };
      const reason = facts.turn_reason ? TURN_REASON[facts.turn_reason] : '';
      if (event.kind === 'turn_cancel_requested')
        return { text: reason || msg('A cancel of the turn was recorded.') };
      return {
        text: reason || msg('The authority recorded how the turn ended.'),
      };
    }
    case 'approval': {
      const { about, approved } = subjectOf(facts.approval_kind);
      if (event.kind === 'approval_requested')
        return {
          text: msg(
            'Codex proposed {about} and the authority held it for a decision. The proposal itself is not part of the record.',
          ),
          values: { about },
        };
      if (event.kind === 'approval_accepted')
        return {
          text: msg(
            'The approval of {about} was accepted and the answer was sent to Codex.',
          ),
          values: { about },
        };
      if (event.kind === 'approval_confirmed')
        return {
          text: msg('Codex reported what it did with {approved}.'),
          values: { approved },
        };
      if (event.kind === 'approval_declined')
        return {
          text: msg(
            'The approval of {about} was denied. Nothing was run or changed for it.',
          ),
          values: { about },
        };
      const reason = facts.approval_reason
        ? APPROVAL_REASON[facts.approval_reason]
        : '';
      return {
        text: reason || msg('The authority recorded how the approval ended.'),
      };
    }
    case 'lease': {
      const scopes =
        facts.scopes && facts.scopes.length > 0
          ? listOf(facts.scopes.map((scope) => ({ text: SCOPE[scope] })))
          : null;
      /** The lease's own sentence, then what it allowed when the record says so. */
      const allowed = (lead: Phrase): Phrase =>
        scopes
          ? {
              text: msg('{lead} It allowed the session to {scopes}.'),
              values: { lead, scopes },
            }
          : lead;
      if (event.kind === 'lease_revoked')
        return allowed({
          text:
            facts.revocation === 'session_closed'
              ? msg('The lease ended because its session was closed.')
              : msg('The lease was given back.'),
        });
      if (event.kind === 'lease_expired')
        return allowed({ text: msg('The lease reached its time limit.') });
      return allowed({
        text:
          event.kind === 'lease_renewed'
            ? msg('The authority extended a lease for this session.')
            : msg('The authority granted a lease for this session.'),
      });
    }
    case 'runtime': {
      if (event.kind === 'runtime_desired')
        return {
          text: msg(
            'The authority accepted a request for the managed Codex runtime and recorded what it should be.',
          ),
        };
      if (event.kind === 'runtime_ready')
        return facts.runtime_version
          ? {
              text: msg('Codex {version} started, was checked and is ready.'),
              values: { version: facts.runtime_version },
            }
          : { text: msg('Codex started, was checked and is ready.') };
      const exit: Phrase = {
        text: facts.exit ? EXIT[facts.exit] : msg('The Codex process ended.'),
      };
      return facts.expected === true
        ? {
            text: msg('{exit} The authority meant it to.'),
            values: { exit },
          }
        : exit;
    }
    case 'session':
      switch (event.kind) {
        case 'session_prepared':
          return {
            text: msg(
              'A managed session was created. It has no workspace until it is bound.',
            ),
          };
        case 'session_closed':
          return { text: msg('The session was closed. Closing is final.') };
        case 'session_bound':
          return {
            text: msg(
              'The authority bound the session to a project folder. Which folder is not part of the record.',
            ),
          };
        case 'session_attached':
          return {
            text: msg(
              'The session was attached to the Codex runtime that was running.',
            ),
          };
        case 'binding_indeterminate':
          return {
            text: msg(
              'The authority could not confirm whether Codex created the workspace binding.',
            ),
          };
        default:
          return {
            text: msg(
              'An uncertain binding attempt was given up on explicitly.',
            ),
          };
      }
    case 'authority':
      return {
        text: msg(
          'The authority recorded an event of a kind this version of the console does not know. Nothing else of it is shown.',
        ),
      };
  }
}

/** What lies before the oldest event shown. */
export function startMessage(start: EventsStart): string {
  return start === 'beginning'
    ? msg('This is the beginning of the event stream.')
    : start === 'unavailable'
      ? msg('History before this point is unavailable.')
      : '';
}

export function eventsReadFailure(code: SessionFailureCode): string {
  return code === 'unavailable'
    ? msg('Events could not be read: the authority did not answer.')
    : code === 'busy'
      ? msg(
          'Events could not be read. The authority is busy; try again in a moment.',
        )
      : code === 'changed'
        ? msg('The event history changed while it was being read.')
        : msg(
            'Events could not be read. Nothing is shown rather than something unverified.',
          );
}
export function olderFailure(code: SessionFailureCode): string {
  return code === 'changed'
    ? msg(
        'Older events could not be read: the authority holds another history than the one shown. The newest events were read again.',
      )
    : code === 'unavailable'
      ? msg('Older events could not be read: the authority did not answer.')
      : msg('Older events could not be read.');
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
