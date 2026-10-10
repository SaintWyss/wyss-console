import type {
  LifecycleDiagnostics,
  LifecycleFailure,
  LifecycleView,
} from '../contracts/lifecycle';
import { readLifecycle } from '../desktop/lifecycle-client';
import {
  summarizeAuthority,
  type AuthorityObservation,
  type StatusSummary,
} from './authority';
import type { RuntimeTone } from './runtime';

/** What the shell knows about the lifecycle. One follower for the whole application. */
export type LifecycleObservation =
  | { state: 'pending' }
  | { state: 'observed'; view: LifecycleView }
  /** The desktop bridge failed or answered something that is not a view. */
  | { state: 'unavailable' };

export const pendingLifecycle: LifecycleObservation = { state: 'pending' };

const RETRY_MS = 1000;
/** Floor between reads that brought nothing new, whatever the bridge does. */
const UNCHANGED_MS = 250;

export interface FollowLifecycleOptions {
  read?: (after: number | null) => Promise<LifecycleView>;
}

/**
 * Follows the desktop's lifecycle view until disposed. One read is in flight at a time:
 * it waits in the desktop until the view changes, so nothing is polled while the
 * authority is ready. It starts nothing: the desktop decides when to act.
 */
export function followLifecycle(
  observe: (observation: LifecycleObservation) => void,
  { read = readLifecycle }: FollowLifecycleOptions = {},
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

const PROGRESS: Partial<
  Record<LifecycleView['phase'], Pick<StatusSummary, 'label' | 'detail'>>
> = {
  installing: {
    label: 'Installing',
    detail: 'Installing the Wyss authority for this account.',
  },
  starting: {
    label: 'Starting',
    detail: 'Starting the Wyss authority.',
  },
  connecting: {
    label: 'Starting',
    detail: 'The Wyss authority is starting. Waiting for it to answer.',
  },
};

/**
 * The top-bar status of the authority. Connected is the link's word and only the
 * link's: the lifecycle adds what is being done while nothing answers, and why nothing
 * does. A lifecycle that says `ready` never shows a connection the link does not have.
 */
export function summarizeLink(
  authority: AuthorityObservation,
  lifecycle: LifecycleObservation,
): StatusSummary {
  const link = summarizeAuthority(authority);
  const connected =
    authority.state === 'observed' &&
    (authority.view.connection === 'connected' ||
      authority.view.connection === 'synchronizing');
  if (lifecycle.state !== 'observed') return link;
  const { view } = lifecycle;
  if (connected) {
    if (authority.view.connection !== 'connected' || view.phase !== 'ready')
      return link;
    const notes = [
      view.origin === 'managed'
        ? 'started by Wyss Console'
        : view.origin === 'external'
          ? 'started outside Wyss Console'
          : null,
      view.update_pending
        ? 'a newer authority is installed the next time it starts'
        : null,
    ].filter((note) => note !== null);
    return {
      ...link,
      detail: [link.detail, ...notes].join(' · '),
      note: view.update_pending ? 'update pending' : link.note,
    };
  }
  const progress = PROGRESS[view.phase];
  if (progress) return { key: link.key, tone: 'pending', ...progress };
  if (view.phase === 'incompatible')
    return {
      key: link.key,
      label: 'Incompatible',
      tone: 'error',
      detail: 'An incompatible Wyss authority is running.',
    };
  if (view.phase === 'failed' && view.failure !== null) {
    const quiet = view.failure === 'stopped' || view.failure === 'not_packaged';
    return {
      key: link.key,
      label: quiet ? 'Offline' : 'Failed to start',
      tone: quiet ? 'idle' : 'error',
      detail: FAILURE[view.failure],
    };
  }
  return link;
}

const FAILURE: Record<LifecycleFailure, string> = {
  not_packaged:
    'No Wyss authority is running. This is a development build: it does not install or start one. Start wyss-control and it is used as soon as it answers.',
  package_invalid:
    'The authority packaged with this application is missing or is not the one it was built with. Nothing was installed. Reinstall Wyss Console.',
  installation_unsafe:
    'The authority was not installed: the place it is installed in is not safely yours (a link, another owner, or a folder others can write to). Nothing was changed.',
  installation_mismatch:
    'A service definition Wyss Console did not write is in the place of its own. It was left as it is, and the authority was not started.',
  install_failed:
    'The authority could not be installed. Nothing is half installed: what was there before is still in place.',
  manager_unavailable:
    'No user service manager answered, so the authority could not be started. Wyss Console starts its authority through the service manager of your login session.',
  start_refused: 'The service manager did not start the authority.',
  startup_failed: 'The authority was started and ended before it answered.',
  exited_repeatedly:
    'The authority ended repeatedly and is no longer being restarted.',
  stopped:
    'The authority is not running: it was stopped, or it ended and was not restarted.',
  socket_unavailable: 'The authority is running and did not answer in time.',
  socket_unsafe:
    'The control socket of the authority is not private to you, so it is not used.',
};

const BUILD: Record<LifecycleDiagnostics['build'], string> = {
  packaged: 'Packaged: carries its own authority',
  development: 'Development: uses an authority that is already running',
  invalid: 'Packaged, but its authority cannot be found',
};
const INSTALLATION: Record<LifecycleDiagnostics['installation'], string> = {
  unknown: 'Not checked yet',
  absent: 'Not installed',
  current: 'Installed',
  outdated: 'Installed by another version',
  mismatch: 'A service definition that is not Wyss Console’s',
  unsafe: 'Not safely yours',
};
const SERVICE: Record<LifecycleDiagnostics['service'], string> = {
  unknown: 'Not checked yet',
  unavailable: 'No service manager answered',
  not_installed: 'Not installed',
  inactive: 'Not running',
  starting: 'Starting',
  running: 'Running',
  failed: 'Failed',
};

export interface LifecycleNotice {
  tone: RuntimeTone;
  title: string;
  text: string;
  /** Retry is offered. */
  retry: boolean;
  /** What a person troubleshooting needs. Words, never a path, a command or a process. */
  details: { term: string; value: string }[];
}

/**
 * What the shell says when the authority is not available and is not becoming so: an
 * attempt that ended, or an authority this build cannot work with. While it is being
 * installed or started, and once it is ready, there is nothing to say here.
 */
export function lifecycleNotice(
  lifecycle: LifecycleObservation,
  authority: AuthorityObservation,
): LifecycleNotice | null {
  if (lifecycle.state !== 'observed') return null;
  const { view } = lifecycle;
  // The link is the one that says connected. While it is, nothing failed.
  if (
    authority.state === 'observed' &&
    authority.view.connection === 'connected'
  )
    return null;
  const details = [
    { term: 'This build', value: BUILD[view.diagnostics.build] },
    {
      term: 'Installation',
      value: INSTALLATION[view.diagnostics.installation],
    },
    { term: 'Service', value: SERVICE[view.diagnostics.service] },
  ];
  if (view.phase === 'incompatible')
    return {
      tone: 'error',
      title: 'An incompatible Wyss authority is running',
      text:
        view.origin === 'managed'
          ? 'It is this installation’s own, from another version. It is replaced the next time it starts; it is not restarted under work that may be running.'
          : 'It was not started by this installation, so it is left exactly as it is. Wyss Console cannot work with it.',
      retry: false,
      details,
    };
  if (view.phase !== 'failed' || view.failure === null) return null;
  const quiet = view.failure === 'stopped' || view.failure === 'not_packaged';
  return {
    tone: quiet ? 'idle' : 'error',
    title: quiet
      ? 'The Wyss authority is not running'
      : 'The Wyss authority could not start',
    text: FAILURE[view.failure],
    retry: view.retry,
    details,
  };
}
