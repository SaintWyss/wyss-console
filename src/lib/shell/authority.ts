import { msg, type Phrase } from '../i18n/msg';
import type {
  AgentActualState,
  AuthorityFault,
  AuthorityView,
} from '../contracts/authority';
import { readAuthorityState } from '../desktop/authority-client';
import type { RuntimeTone } from './runtime';

/** What the shell knows about the authority. Independent of the terminal workspace. */
export type AuthorityObservation =
  | { state: 'pending' }
  | { state: 'observed'; view: AuthorityView }
  /** The desktop bridge failed or answered something that is not a view. */
  | { state: 'unavailable' };

export const pendingAuthority: AuthorityObservation = { state: 'pending' };

export interface StatusSummary {
  /** Names what the status is about. */
  key: string;
  label: string;
  tone: RuntimeTone;
  /** One sentence for the tooltip, in parts that are drawn joined. */
  detail: Phrase[];
  /** Secondary text shown next to the label when there is room. */
  note?: Phrase;
}

const RETRY_MS = 1000;
/** Floor between reads that brought nothing new, whatever the bridge does. */
const UNCHANGED_MS = 250;

export interface FollowOptions {
  read?: (after: number | null) => Promise<AuthorityView>;
}

/**
 * Follows the desktop's authority projection until disposed or until the service
 * reports it stopped. One read is in flight at a time; a failed read is retried.
 */
export function followAuthority(
  observe: (observation: AuthorityObservation) => void,
  { read = readAuthorityState }: FollowOptions = {},
): () => void {
  let active = true;
  let after: number | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const later = (ms: number) => {
    timer = setTimeout(step, ms);
  };
  function step() {
    timer = undefined;
    if (!active) return;
    read(after).then(
      (view) => {
        if (!active) return;
        const changed = view.revision !== after;
        after = view.revision;
        if (changed) observe({ state: 'observed', view });
        if (view.connection === 'stopped') return;
        if (changed) step();
        else later(UNCHANGED_MS);
      },
      () => {
        if (!active) return;
        after = null;
        observe({ state: 'unavailable' });
        later(RETRY_MS);
      },
    );
  }
  step();
  return () => {
    active = false;
    if (timer !== undefined) clearTimeout(timer);
  };
}

const FAULTS: Record<
  AuthorityFault,
  Pick<StatusSummary, 'label' | 'tone' | 'detail'>
> = {
  absent: {
    label: msg('Offline'),
    tone: 'idle',
    detail: [
      {
        text: msg(
          'The authority is not running. Local terminals do not depend on it.',
        ),
      },
    ],
  },
  lost: {
    label: msg('Reconnecting'),
    tone: 'warn',
    detail: [{ text: msg('The authority stopped answering. Reconnecting.') }],
  },
  incompatible: {
    label: msg('Incompatible'),
    tone: 'error',
    detail: [
      {
        text: msg('The authority speaks another protocol or storage version.'),
      },
    ],
  },
  malformed: {
    label: msg('Rejected'),
    tone: 'error',
    detail: [
      {
        text: msg(
          'The authority broke the control protocol and was disconnected.',
        ),
      },
    ],
  },
  unsafe_path: {
    label: msg('Unsafe'),
    tone: 'error',
    detail: [
      {
        text: msg('The runtime directory or control socket is not private.'),
      },
    ],
  },
};

/** A count with the word for it, in the window's one and many forms. */
function counted(count: number, one: string, many: string): Phrase {
  return { text: count === 1 ? one : many, values: { count } };
}

/** Presentation only: the link to the authority as one top-bar status. */
export function summarizeAuthority(
  observation: AuthorityObservation,
): StatusSummary {
  const key = 'Authority';
  if (observation.state === 'pending')
    return {
      key,
      label: msg('Connecting'),
      tone: 'pending',
      detail: [{ text: msg('Looking for the Wyss authority.') }],
    };
  if (observation.state === 'unavailable')
    return {
      key,
      label: msg('Unavailable'),
      tone: 'error',
      detail: [
        { text: msg('The desktop could not read the authority state.') },
      ],
    };
  const { view } = observation;
  if (view.connection === 'connected' && view.authority) {
    const a = view.authority;
    return {
      key,
      label: msg('Connected'),
      tone: 'ok',
      note: counted(
        a.prepared_sessions,
        msg('{count} session'),
        msg('{count} sessions'),
      ),
      detail: [
        a.service_version
          ? {
              text: msg('Wyss authority {version}'),
              values: { version: a.service_version },
            }
          : { text: msg('Wyss authority') },
        {
          text: msg('protocol {protocol}'),
          values: { protocol: a.protocol_version },
        },
        {
          text: msg('sessions: {prepared} prepared, {closed} closed'),
          values: {
            prepared: a.prepared_sessions,
            closed: a.closed_sessions,
          },
        },
        {
          text: msg('event head {sequence}'),
          values: { sequence: a.last_sequence ?? '0' },
        },
      ],
    };
  }
  if (view.connection === 'synchronizing')
    return {
      key,
      label: msg('Syncing'),
      tone: 'pending',
      detail: [{ text: msg('Reading the canonical authority state.') }],
    };
  if (view.connection === 'stopped')
    return {
      key,
      label: msg('Stopped'),
      tone: 'idle',
      detail: [{ text: msg('The console is closing its authority link.') }],
    };
  if (view.reason) return { key, ...FAULTS[view.reason] };
  return {
    key,
    label: msg('Connecting'),
    tone: 'pending',
    detail: [{ text: msg('Looking for the Wyss authority.') }],
  };
}

const AGENT: Record<AgentActualState, Pick<StatusSummary, 'label' | 'tone'>> = {
  ready: { label: msg('Ready'), tone: 'ok' },
  starting: { label: msg('Starting'), tone: 'pending' },
  initializing: { label: msg('Initializing'), tone: 'pending' },
  stopping: { label: msg('Stopping'), tone: 'pending' },
  stopped: { label: msg('Stopped'), tone: 'idle' },
  failed: { label: msg('Failed'), tone: 'error' },
};

/** Presentation only: the managed agent runtime, as the authority reports it. */
export function summarizeAgent(
  observation: AuthorityObservation,
): StatusSummary {
  const agent =
    observation.state === 'observed' ? observation.view.agent : null;
  // The status keeps one name whether or not anything is known about it: Codex is the one
  // agent this product manages. Only its state is unknown without the authority.
  if (!agent)
    return {
      key: 'Codex',
      label: msg('Unknown'),
      tone: 'idle',
      detail: [
        {
          text: msg(
            'Codex status is known only while the authority is connected.',
          ),
        },
      ],
    };
  const name = agent.adapter === 'codex' ? 'Codex' : agent.adapter;
  return {
    key: name,
    ...AGENT[agent.actual],
    detail: [
      agent.runtime_version
        ? {
            text: msg('{name} {version}'),
            values: { name, version: agent.runtime_version },
          }
        : { text: msg('{name}'), values: { name } },
      { text: msg('desired {state}'), values: { state: agent.desired } },
      counted(agent.model_count, msg('{count} model'), msg('{count} models')),
      { text: msg('managed by the authority') },
    ],
  };
}
