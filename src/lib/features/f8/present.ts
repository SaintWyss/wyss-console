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
  stopped: { label: 'Stopped', tone: 'idle' },
  starting: { label: 'Starting', tone: 'pending' },
  initializing: { label: 'Initializing', tone: 'pending' },
  ready: { label: 'Ready', tone: 'ok' },
  stopping: { label: 'Stopping', tone: 'pending' },
  failed: { label: 'Failed', tone: 'error' },
};

const DIAGNOSTIC: Record<RuntimeDiagnostic, string> = {
  unavailable: 'The runtime could not be found.',
  spawn_failed: 'The runtime process could not be started.',
  transport_failed: 'The connection to the runtime broke.',
  handshake_timeout: 'The runtime did not finish starting in time.',
  protocol_failed:
    'The runtime answered in a way the authority does not accept.',
  server_request: 'The runtime asked for something it may not ask for.',
  runtime_rejected: 'The runtime refused to start.',
  catalog_rejected: 'The runtime listed models the authority does not accept.',
  child_exited: 'The runtime process exited.',
  audit_unavailable: 'The authority could not record what the runtime did.',
  inference_isolation_violation:
    'The runtime tried to act outside what its session allows, and the authority stopped it.',
  safety_unproven:
    'The authority could not prove the runtime was isolated, so it did not use it.',
};
export function diagnosticText(diagnostic: RuntimeDiagnostic): string {
  return DIAGNOSTIC[diagnostic];
}

/** What a resource budget ended the runtime for, from its closed key. */
export function budgetText(key: string): string {
  return `A resource limit ended it: ${key.replace(/_/g, ' ')}.`;
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
  unbound: { label: 'No workspace', tone: 'idle' },
  creating: { label: 'Binding…', tone: 'pending' },
  indeterminate: { label: 'Outcome unknown', tone: 'warn' },
  bound_detached: { label: 'Workspace bound', tone: 'idle' },
  bound_attached: { label: 'Attached', tone: 'ok' },
};

export const SCOPE_LABEL: Record<LeaseScope, string> = {
  session_create: 'Bind a workspace',
  session_resume: 'Resume a binding',
  turn_start: 'Start turns',
  command_execute: 'Run commands',
  file_change: 'Change files',
};
export const SCOPE_NOTE: Record<LeaseScope, string> = {
  session_create: 'Lets the session be bound to a workspace folder.',
  session_resume:
    'Lets a bound session be attached to a running runtime again.',
  turn_start: 'Lets a turn start. Without it nothing runs.',
  command_execute:
    'Lets Codex ask to run a command. Each one still waits for you.',
  file_change:
    'Lets Codex ask to change files. Each change still waits for you.',
};

export function ttlLabel(seconds: number): string {
  if (seconds < 3600) return `${seconds / 60} min`;
  return `${seconds / 3600} h`;
}

/** Why the execution gate says no, in one sentence. */
const GATE: Record<GateReasonName, string> = {
  allowed: 'Allowed.',
  binding_not_found: 'The session has no workspace.',
  binding_detached: 'The session is not attached to a running runtime.',
  attachment_runtime_mismatch:
    'The session is attached to a different runtime launch.',
  inference_profile_missing:
    'The session is not attached in a mode that allows this.',
  local_tools_profile_missing: 'The session is not attached in Local Tools.',
  runtime_safety_profile_missing: 'The runtime has no safety profile.',
  active_turn_exists: 'A turn is already running.',
  session_not_found: 'The authority has no such session.',
  session_not_prepared: 'The session is closed.',
  lease_missing: 'There is no execution lease.',
  lease_not_active: 'The execution lease is not active.',
  lease_expired: 'The execution lease expired.',
  scope_not_granted: 'The execution lease does not grant this.',
  runtime_mismatch: 'The runtime is not the session’s.',
  runtime_not_running: 'The runtime is not running.',
  runtime_not_ready: 'The runtime is not ready.',
  runtime_identity_missing: 'The runtime has not been attested yet.',
  runtime_incompatible: 'The runtime is not a version the authority accepts.',
  authority_unavailable: 'The authority is unavailable.',
};
export function gateText(reason: GateReasonName): string {
  return GATE[reason];
}

const FAILURE: Record<F8FailureCode, string> = {
  inactive: 'This console is not connected to managed runtimes.',
  unavailable: 'The authority did not answer, so the outcome is not known.',
  incompatible: 'This console and the authority speak different versions.',
  invalid: 'The console sent a request it should not have. Nothing changed.',
  not_found: 'The authority no longer has it.',
  stale:
    'What was shown is no longer what the authority holds. It has been read again.',
  not_allowed: 'The authority refused: it is no longer allowed.',
  not_bound: 'The session has no workspace yet.',
  runtime_unavailable: 'The session has no ready runtime for this.',
  turn_active: 'A turn is running. Nothing is stopped under it.',
  workspace_invalid: 'The authority did not accept the folder as a workspace.',
  already_bound: 'The session already has a workspace.',
  permission_denied:
    'The authority denied it: no execution lease allows this action.',
  lease_active: 'The session already has an active execution lease.',
  busy: 'The authority is busy with another operation. Try again in a moment.',
  rejected: 'The agent runtime refused.',
  uncertain:
    'The authority could not confirm what the runtime did. Nothing is sent again by itself.',
  conflict: 'That request was already used for something else.',
  internal:
    'Something unexpected happened. Nothing is assumed to have changed.',
};
export function failureText(code: F8FailureCode): string {
  return FAILURE[code];
}
/** The outcome of the action is not known: it may have happened. */
export function outcomeUnknown(code: F8FailureCode): boolean {
  return code === 'unavailable' || code === 'uncertain';
}

const FAULT: Record<LinkFault, string> = {
  absent: 'No managed-runtime authority is listening.',
  lost: 'The authority stopped answering.',
  incompatible: 'The authority speaks another protocol or storage version.',
  malformed: 'The authority broke the protocol, so the connection was dropped.',
  unsafe_path: 'The authority’s socket is not this user’s private one.',
};
export function linkText(link: LinkInfo): {
  label: string;
  tone: RuntimeTone;
  note: string | null;
} {
  switch (link.state) {
    case 'connected':
      return { label: 'Connected', tone: 'ok', note: null };
    case 'connecting':
    case 'synchronizing':
      return { label: 'Connecting', tone: 'pending', note: null };
    case 'waiting':
      return {
        label: 'Offline',
        tone: 'warn',
        note: link.fault ? FAULT[link.fault] : null,
      };
    case 'stopped':
      return { label: 'Stopped', tone: 'idle', note: null };
  }
}

/** When a lease ends, in the user's clock. */
export function leaseLine(lease: LeaseInfo, now: number): string {
  if (lease.state === 'revoked') return 'Revoked';
  const left = Date.parse(lease.expires_at) - now;
  if (lease.state === 'expired' || left <= 0) return 'Expired';
  const seconds = Math.ceil(left / 1000);
  if (seconds < 90) return `Expires in ${seconds} s`;
  const minutes = Math.ceil(seconds / 60);
  return minutes < 90
    ? `Expires in ${minutes} min`
    : `Expires in ${Math.ceil(minutes / 60)} h`;
}

/** The runtime row, in one line. */
export function runtimeLine(runtime: RuntimeInfo): string {
  const state = RUNTIME_STATE[runtime.actual].label;
  if (runtime.generation === 0) return 'No runtime yet';
  const profile = runtime.profile ? ` · ${PROFILE_LABEL[runtime.profile]}` : '';
  return `${state} · generation ${runtime.generation}${profile}`;
}
