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
  /** One sentence for the tooltip. */
  detail: string;
  /** Secondary text shown next to the label when there is room. */
  note?: string;
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
    label: 'Offline',
    tone: 'idle',
    detail: 'wyss-control is not running. Local terminals do not depend on it.',
  },
  lost: {
    label: 'Reconnecting',
    tone: 'warn',
    detail: 'The authority stopped answering. Reconnecting.',
  },
  incompatible: {
    label: 'Incompatible',
    tone: 'error',
    detail: 'The authority speaks another protocol or storage version.',
  },
  malformed: {
    label: 'Rejected',
    tone: 'error',
    detail: 'The authority broke the control protocol and was disconnected.',
  },
  unsafe_path: {
    label: 'Unsafe',
    tone: 'error',
    detail: 'The runtime directory or control socket is not private.',
  },
};

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

/** Presentation only: the link to the authority as one top-bar status. */
export function summarizeAuthority(
  observation: AuthorityObservation,
): StatusSummary {
  const key = 'Authority';
  if (observation.state === 'pending')
    return {
      key,
      label: 'Connecting',
      tone: 'pending',
      detail: 'Looking for wyss-control.',
    };
  if (observation.state === 'unavailable')
    return {
      key,
      label: 'Unavailable',
      tone: 'error',
      detail: 'The desktop could not read the authority state.',
    };
  const { view } = observation;
  if (view.connection === 'connected' && view.authority) {
    const a = view.authority;
    const sessions = `${a.prepared_sessions} prepared, ${a.closed_sessions} closed`;
    return {
      key,
      label: 'Connected',
      tone: 'ok',
      note: plural(a.prepared_sessions, 'session'),
      detail: [
        `wyss-control${a.service_version ? ` ${a.service_version}` : ''}`,
        `protocol ${a.protocol_version}`,
        `sessions: ${sessions}`,
        `event head ${a.last_sequence ?? '0'}`,
      ].join(' · '),
    };
  }
  if (view.connection === 'synchronizing')
    return {
      key,
      label: 'Syncing',
      tone: 'pending',
      detail: 'Reading the canonical authority state.',
    };
  if (view.connection === 'stopped')
    return {
      key,
      label: 'Stopped',
      tone: 'idle',
      detail: 'The console is closing its authority link.',
    };
  if (view.reason) return { key, ...FAULTS[view.reason] };
  return {
    key,
    label: 'Connecting',
    tone: 'pending',
    detail: 'Looking for wyss-control.',
  };
}

const AGENT: Record<AgentActualState, Pick<StatusSummary, 'label' | 'tone'>> = {
  ready: { label: 'Ready', tone: 'ok' },
  starting: { label: 'Starting', tone: 'pending' },
  initializing: { label: 'Initializing', tone: 'pending' },
  stopping: { label: 'Stopping', tone: 'pending' },
  stopped: { label: 'Stopped', tone: 'idle' },
  failed: { label: 'Failed', tone: 'error' },
};

/** Presentation only: the managed agent runtime, as the authority reports it. */
export function summarizeAgent(
  observation: AuthorityObservation,
): StatusSummary {
  const agent =
    observation.state === 'observed' ? observation.view.agent : null;
  if (!agent)
    return {
      key: 'Agent',
      label: 'Unknown',
      tone: 'idle',
      detail:
        'Managed agent status is known only while the authority is connected.',
    };
  const name = agent.adapter === 'codex' ? 'Codex' : agent.adapter;
  return {
    key: name,
    ...AGENT[agent.actual],
    detail: [
      `${name}${agent.runtime_version ? ` ${agent.runtime_version}` : ''}`,
      `desired ${agent.desired}`,
      plural(agent.model_count, 'model'),
      'managed by the authority',
    ].join(' · '),
  };
}
