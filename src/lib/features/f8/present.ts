import type {
  ActualState,
  AttachmentProfile,
  BindingStateName,
  F8FailureCode,
  GateReasonName,
  LeaseInfo,
  LeaseScope,
  LinkFault,
  LinkInfo,
  RuntimeDiagnostic,
  RuntimeInfo,
} from '../../contracts/f8';
import { msg, type Phrase } from '../../i18n/msg';
import type { RuntimeTone } from '../../shell/runtime';

/** Presentation only: nothing here is read back as state. */

/**
 * Text from a provider (a command, a directory, a path, a diff) as a person reads it before
 * approving: every format and bidirectional control character is shown as its code point, so a
 * right-to-left override or an invisible character cannot make one thing read as another.
 */
export function visible(text: string): string {
  return text.replace(
    /[\p{Cf}\u2028\u2029]/gu,
    (c) =>
      `⟨U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, '0')}⟩`,
  );
}

export const RUNTIME_STATE: Record<
  ActualState,
  { label: string; tone: RuntimeTone }
> = {
  stopped: { label: msg('Stopped'), tone: 'idle' },
  starting: { label: msg('Starting'), tone: 'pending' },
  initializing: { label: msg('Initializing'), tone: 'pending' },
  ready: { label: msg('Ready'), tone: 'ok' },
  stopping: { label: msg('Stopping'), tone: 'pending' },
  failed: { label: msg('Failed'), tone: 'error' },
};

const DIAGNOSTIC: Record<RuntimeDiagnostic, string> = {
  unavailable: msg('The runtime could not be found.'),
  spawn_failed: msg('The runtime process could not be started.'),
  transport_failed: msg('The connection to the runtime broke.'),
  handshake_timeout: msg('The runtime did not finish starting in time.'),
  protocol_failed: msg(
    'The runtime answered in a way the authority does not accept.',
  ),
  server_request: msg('The runtime asked for something it may not ask for.'),
  runtime_rejected: msg('The runtime refused to start.'),
  catalog_rejected: msg(
    'The runtime listed models the authority does not accept.',
  ),
  child_exited: msg('The runtime process exited.'),
  audit_unavailable: msg(
    'The authority could not record what the runtime did.',
  ),
  inference_isolation_violation: msg(
    'The runtime tried to act outside what its session allows, and the authority stopped it.',
  ),
  safety_unproven: msg(
    'The authority could not prove the runtime was isolated, so it did not use it.',
  ),
};
export function diagnosticText(diagnostic: RuntimeDiagnostic): string {
  return DIAGNOSTIC[diagnostic];
}

/** What a resource budget ended the runtime for, from its closed key. */
export function budgetText(key: string): Phrase {
  return {
    text: msg('A resource limit ended it: {key}.'),
    values: { key: key.replace(/_/g, ' ') },
  };
}

export const PROFILE_LABEL = {
  codex_inference_v1: 'Inference',
  codex_local_tools_v1: 'Local Tools',
} as const;
export const ATTACHMENT_LABEL: Record<AttachmentProfile, string> = {
  inference_only_v1: 'Inference',
  local_tools_v1: 'Local Tools',
};

export const BINDING_STATE: Record<
  BindingStateName,
  { label: string; tone: RuntimeTone }
> = {
  unbound: { label: msg('No workspace'), tone: 'idle' },
  creating: { label: msg('Binding…'), tone: 'pending' },
  indeterminate: { label: msg('Outcome unknown'), tone: 'warn' },
  bound_detached: { label: msg('Workspace bound'), tone: 'idle' },
  bound_attached: { label: msg('Attached'), tone: 'ok' },
};

export const SCOPE_LABEL: Record<LeaseScope, string> = {
  session_create: msg('Bind a workspace'),
  session_resume: msg('Resume a binding'),
  turn_start: msg('Start turns'),
  command_execute: msg('Run commands'),
  file_change: msg('Change files'),
};
export const SCOPE_NOTE: Record<LeaseScope, string> = {
  session_create: msg('Lets the session be bound to a workspace folder.'),
  session_resume: msg(
    'Lets a bound session be attached to a running runtime again.',
  ),
  turn_start: msg('Lets a turn start. Without it nothing runs.'),
  command_execute: msg(
    'Lets Codex ask to run a command. Each one still waits for you.',
  ),
  file_change: msg(
    'Lets Codex ask to change files. Each change still waits for you.',
  ),
};

export function ttlLabel(seconds: number): Phrase {
  if (seconds < 3600)
    return { text: msg('{minutes} min'), values: { minutes: seconds / 60 } };
  return { text: msg('{hours} h'), values: { hours: seconds / 3600 } };
}

/** Why the execution gate says no, in one sentence. */
const GATE: Record<GateReasonName, string> = {
  allowed: msg('Allowed.'),
  binding_not_found: msg('The session has no workspace.'),
  binding_detached: msg('The session is not attached to a running runtime.'),
  attachment_runtime_mismatch: msg(
    'The session is attached to a different runtime launch.',
  ),
  inference_profile_missing: msg(
    'The session is not attached in a mode that allows this.',
  ),
  local_tools_profile_missing: msg(
    'The session is not attached in Local Tools.',
  ),
  runtime_safety_profile_missing: msg('The runtime has no safety profile.'),
  active_turn_exists: msg('A turn is already running.'),
  session_not_found: msg('The authority has no such session.'),
  session_not_prepared: msg('The session is closed.'),
  lease_missing: msg('There is no execution lease.'),
  lease_not_active: msg('The execution lease is not active.'),
  lease_expired: msg('The execution lease expired.'),
  scope_not_granted: msg('The execution lease does not grant this.'),
  runtime_mismatch: msg('The runtime is not the session’s.'),
  runtime_not_running: msg('The runtime is not running.'),
  runtime_not_ready: msg('The runtime is not ready.'),
  runtime_identity_missing: msg('The runtime has not been attested yet.'),
  runtime_incompatible: msg(
    'The runtime is not a version the authority accepts.',
  ),
  authority_unavailable: msg('The authority is unavailable.'),
};
export function gateText(reason: GateReasonName): string {
  return GATE[reason];
}

const FAILURE: Record<F8FailureCode, string> = {
  inactive: msg('This console is not connected to managed runtimes.'),
  unavailable: msg(
    'The authority did not answer, so the outcome is not known. Nothing was canceled and it may still have happened. The console reads the authority again; check what it holds before sending this again.',
  ),
  incompatible: msg('This console and the authority speak different versions.'),
  invalid: msg(
    'The console sent a request it should not have. Nothing changed.',
  ),
  unsupported: msg(
    'This authority does not offer that. Nothing changed; try another mode.',
  ),
  not_found: msg('The authority no longer has it.'),
  stale: msg(
    'What was shown is no longer what the authority holds. It has been read again.',
  ),
  not_allowed: msg('The authority refused: it is no longer allowed.'),
  not_bound: msg('The session has no workspace yet.'),
  runtime_unavailable: msg('The session has no ready runtime for this.'),
  turn_active: msg('A turn is running. Nothing is stopped under it.'),
  workspace_invalid: msg(
    'The authority did not accept the folder as a workspace.',
  ),
  already_bound: msg('The session already has a workspace.'),
  permission_denied: msg(
    'The authority denied it: no execution lease allows this action.',
  ),
  lease_active: msg('The session already has an active execution lease.'),
  busy: msg(
    'The authority is busy with another operation. Try again in a moment.',
  ),
  capacity: msg(
    'The authority is at its limit for this and did not take it. Nothing changed. Only one runtime runs at a time: stop the one that is running, then start this one.',
  ),
  rejected: msg('The agent runtime refused.'),
  uncertain: msg(
    'The authority could not confirm what the runtime did. Nothing is sent again by itself.',
  ),
  conflict: msg('That request was already used for something else.'),
  not_confirmed: msg(
    'It was not allowed in the confirmation window, so nothing was sent.',
  ),
  internal: msg(
    'Something unexpected happened. Nothing is assumed to have changed.',
  ),
};
export function failureText(code: F8FailureCode): string {
  return FAILURE[code];
}
/** The outcome of the action is not known: it may have happened. */
export function outcomeUnknown(code: F8FailureCode): boolean {
  return code === 'unavailable' || code === 'uncertain';
}

const FAULT: Record<LinkFault, string> = {
  absent: msg('No managed-runtime authority is listening.'),
  lost: msg('The authority stopped answering.'),
  incompatible: msg(
    'The authority speaks another protocol or storage version.',
  ),
  malformed: msg(
    'The authority broke the protocol, so the connection was dropped.',
  ),
  unsafe_path: msg('The authority’s socket is not this user’s private one.'),
};
export function linkText(link: LinkInfo): {
  label: string;
  tone: RuntimeTone;
  note: string | null;
} {
  switch (link.state) {
    case 'connected':
      return { label: msg('Connected'), tone: 'ok', note: null };
    case 'connecting':
    case 'synchronizing':
      return { label: msg('Connecting'), tone: 'pending', note: null };
    case 'waiting':
      return {
        label: msg('Offline'),
        tone: 'warn',
        note: link.fault ? FAULT[link.fault] : null,
      };
    case 'stopped':
      return { label: msg('Stopped'), tone: 'idle', note: null };
  }
}

/** When a lease ends, in the user's clock. */
export function leaseLine(lease: LeaseInfo, now: number): Phrase {
  if (lease.state === 'revoked') return { text: msg('Revoked') };
  const left = Date.parse(lease.expires_at) - now;
  if (lease.state === 'expired' || left <= 0) return { text: msg('Expired') };
  const seconds = Math.ceil(left / 1000);
  if (seconds < 90)
    return { text: msg('Expires in {n} s'), values: { n: seconds } };
  const minutes = Math.ceil(seconds / 60);
  return minutes < 90
    ? { text: msg('Expires in {n} min'), values: { n: minutes } }
    : { text: msg('Expires in {n} h'), values: { n: Math.ceil(minutes / 60) } };
}

/** The runtime row, in one line. */
export function runtimeLine(runtime: RuntimeInfo): Phrase {
  if (runtime.generation === 0) return { text: msg('No runtime yet') };
  const profile = runtime.profile ? ` · ${PROFILE_LABEL[runtime.profile]}` : '';
  return {
    text: msg('{state} · generation {generation}{profile}'),
    values: {
      state: { text: RUNTIME_STATE[runtime.actual].label },
      generation: runtime.generation,
      profile,
    },
  };
}
