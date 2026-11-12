import type {
  Availability,
  Aggregate,
  EffortInfo,
  RerouteInfo,
  Tokens,
} from '../../contracts/m3';
import { msg, type Phrase } from '../../i18n/msg';

/** Presentation only: nothing here is read back as state. */

/** Text from a provider as a person reads it before approving: format and bidi characters shown. */
export function visible(text: string): string {
  return text.replace(
    /[\p{Cf}\u2028\u2029]/gu,
    (c) =>
      `\u27e8U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, '0')}\u27e9`,
  );
}

export const AVAILABILITY_LABEL: Record<
  Availability,
  { label: string; tone: 'ok' | 'pending' | 'error' | 'idle' }
> = {
  available: { label: msg('Available'), tone: 'ok' },
  unverified: { label: msg('Unverified'), tone: 'pending' },
  blocked: { label: msg('Blocked'), tone: 'error' },
  unsupported: { label: msg('Unsupported'), tone: 'idle' },
};

/** Every reason code the projection can carry, in words. An unknown code is shown as itself. */
const REASON: Record<string, string> = {
  adr0053_not_accepted: msg(
    'The decision on Claude Managed security and authentication has not been accepted.',
  ),
  store_cannot_hold_claude: msg(
    'The authority’s store only accepts Codex runtimes; holding a Claude runtime needs a store change the owner has not approved.',
  ),
  real_provider_evidence_missing: msg(
    'No authorized run against the real provider has been recorded for this profile.',
  ),
  credential_delivery_not_accepted: msg(
    'How a provider key would reach a Claude runtime has not been decided; keys stay sealed and none is delivered.',
  ),
  independent_review_missing: msg(
    'No reviewer other than the author has inspected this runtime and its network confinement.',
  ),
  adr0054_not_accepted: msg(
    'Choosing a model or reasoning effort for a Managed turn needs a decision the owner has not accepted.',
  ),
  metrics_policy_not_accepted: msg(
    'The privacy and retention policy for a usage history has not been accepted.',
  ),
  sdk_not_configured: msg(
    'No Claude Agent SDK installation was named for this Console.',
  ),
  authority_not_connected: msg('The protocol-2 authority is not connected.'),
  platform_unsupported: msg('This platform has no accepted evidence.'),
  verifying: msg('The installation is being checked.'),
  sdk_version_unsupported: msg(
    'The SDK version is not the one this build was qualified against.',
  ),
  cli_version_unsupported: msg(
    'The bundled Claude Code version is not the qualified one.',
  ),
  cli_digest_mismatch: msg(
    'The bundled Claude Code executable is not the pinned one.',
  ),
  record_mismatch: msg(
    'A file of the SDK installation does not match what pip recorded.',
  ),
  unlisted_file: msg(
    'The SDK installation holds a file no package vouches for.',
  ),
  managed_policy_present: msg(
    'An administrator’s Claude Code policy exists on this machine; it can add hooks and tools no option turns off.',
  ),
  credential_unusable: msg(
    'The credential file is not private, plain and non-empty.',
  ),
  unsafe_path: msg('A path of the installation is not safe to use.'),
  not_a_virtual_environment: msg(
    'The named folder is not an isolated Python virtual environment.',
  ),
};
export function reasonText(code: string): string {
  return REASON[code] ?? code;
}

/** A reported count; a missing one is "not reported", never zero. */
export function counter(value: number | null): string {
  return value === null ? 'not reported' : value.toLocaleString('en-US');
}

/**
 * A running total over turns. It is exact only when every turn reported the counter; otherwise it
 * says how many did not.
 */
export function running(a: Aggregate): string {
  if (a.reported_turns === 0 && a.unreported_turns === 0) return '—';
  if (a.reported_turns === 0) return 'not reported';
  return a.unreported_turns === 0
    ? a.sum.toLocaleString('en-US')
    : `at least ${a.sum.toLocaleString('en-US')} (${a.unreported_turns} turn${a.unreported_turns === 1 ? '' : 's'} did not report)`;
}

export function tokenLine(t: Tokens): string {
  return [
    `in ${counter(t.input)}`,
    `out ${counter(t.output)}`,
    `cache write ${counter(t.cache_creation)}`,
    `cache read ${counter(t.cache_read)}`,
    `reasoning ${counter(t.reasoning)}`,
  ].join(' · ');
}

export const COMPLETENESS_LABEL: Record<string, string> = {
  complete: msg('final figures from the provider'),
  provider_error: msg('the provider reported an error'),
  interrupted: msg('interrupted: streaming figures only'),
  unreported: msg('the provider reported nothing'),
  running: msg('running'),
};

export function rerouteLine(r: RerouteInfo): Phrase {
  switch (r.state) {
    case 'not_requested':
      return { text: msg('No model was requested for this turn.') };
    case 'not_reported':
      return {
        text: msg('The provider has not named the model that answered.'),
      };
    case 'honored':
      return {
        text: msg('The provider named the model that was requested.'),
      };
    case 'rerouted':
      return {
        text: msg('Requested {requested}; the provider named {reported}.'),
        values: { requested: r.requested, reported: r.reported.join(', ') },
      };
  }
}

export function effortLine(e: EffortInfo): Phrase {
  switch (e.state) {
    case 'not_requested':
      return { text: msg('No effort was requested.') };
    case 'requested':
      return {
        text: msg('Requested {value}. Nothing shows the provider received it.'),
        values: { value: e.value },
      };
    case 'observed_in_provider_request':
      return {
        text: msg(
          '{value} was in the request the provider received. That is not proof it took effect.',
        ),
        values: { value: e.value },
      };
    case 'provider_reported':
      return {
        text: msg('The provider reported {value}.'),
        values: { value: e.value },
      };
  }
}

export function localTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString('en-GB', {
    hour12: false,
  });
}

export const WINDOW_LABEL: Record<string, string> = {
  five_hour: msg('5-hour window'),
  seven_day: msg('7-day window'),
  seven_day_opus: msg('7-day window (Opus)'),
  seven_day_sonnet: msg('7-day window (Sonnet)'),
  overage: msg('Overage'),
};

// ---- a held action, as the window reads it -------------------------------------------------------

export type ActionShape =
  | { kind: 'command'; command: string; cwd: string }
  | {
      kind: 'files';
      changes: {
        path: string;
        change: string;
        move_path: string | null;
        diff: string;
      }[];
    }
  | { kind: 'unknown' };

/** Reads a held action. Anything not exactly a command or a file change is `unknown`, and an
 * unknown action can be declined or cancelled but never accepted. */
export function describeAction(action: unknown): ActionShape {
  if (typeof action !== 'object' || action === null) return { kind: 'unknown' };
  const a = action as Record<string, unknown>;
  if (
    a.kind === 'command' &&
    typeof a.command === 'string' &&
    typeof a.cwd === 'string'
  )
    return { kind: 'command', command: a.command, cwd: a.cwd };
  if (a.kind === 'files' && Array.isArray(a.changes) && a.changes.length > 0) {
    const changes = [];
    for (const c of a.changes) {
      if (
        typeof c !== 'object' ||
        c === null ||
        typeof (c as Record<string, unknown>).path !== 'string' ||
        typeof (c as Record<string, unknown>).diff !== 'string' ||
        typeof (c as Record<string, unknown>).kind !== 'string'
      )
        return { kind: 'unknown' };
      const x = c as Record<string, unknown>;
      changes.push({
        path: x.path as string,
        change: x.kind as string,
        move_path: typeof x.move_path === 'string' ? x.move_path : null,
        diff: x.diff as string,
      });
    }
    return { kind: 'files', changes };
  }
  return { kind: 'unknown' };
}

export const APPROVAL_STATE_LABEL: Record<
  string,
  { label: string; tone: 'ok' | 'pending' | 'error' | 'idle' }
> = {
  pending: { label: msg('Waiting for your decision'), tone: 'pending' },
  accepted: { label: msg('Accepted — running'), tone: 'pending' },
  confirmed: { label: msg('Done'), tone: 'ok' },
  declined: { label: msg('Declined — nothing ran'), tone: 'idle' },
  cancelled: { label: msg('Cancelled — nothing ran'), tone: 'idle' },
  failed: {
    label: msg('Could not be done — nothing changed'),
    tone: 'error',
  },
  indeterminate: {
    label: msg('Unknown whether it happened — it will not be repeated'),
    tone: 'error',
  },
  refused: { label: msg('Refused before asking you'), tone: 'idle' },
};

export const PROFILE_LABEL: Record<string, string> = {
  codex_inference: 'Codex · Inference',
  codex_local_tools: 'Codex · Local tools',
  claude_inference: 'Claude · Inference',
  claude_local_tools: 'Claude · Local tools',
};

export const REFUSAL_LABEL: Record<string, string> = {
  already_running: msg('A fixture runtime is already running.'),
  not_running: msg('There is no running fixture runtime.'),
  not_ready: msg('The runtime is not ready for a prompt.'),
  invalid_prompt: msg('The prompt is empty or too long.'),
  no_active_turn: msg('No turn is running.'),
  not_pending: msg('That action is not waiting for a decision.'),
  invalid_decision: msg('That is not a decision.'),
  decision_refused: msg('The worker refused the decision; it is not retried.'),
  turn_refused: msg('The runtime refused the turn.'),
  start_refused: msg('The runtime could not be started.'),
  unknown_profile: msg('That profile does not exist.'),
  gate_closed: msg('The decision this needs has not been accepted.'),
  unreachable: msg('The window could not reach the service.'),
};
