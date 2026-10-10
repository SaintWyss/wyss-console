import {
  FONT_SIZE,
  type CursorStyle,
  type EventFilterPreference,
  type LandingPage,
  type SettingsFailureCode,
  type StorageState,
  type TimeFormat,
} from '../../contracts/settings';
import {
  summarizeAgent,
  summarizeAuthority,
  type AuthorityObservation,
} from '../../shell/authority';
import type { LifecycleObservation } from '../../shell/lifecycle';
import type { RuntimeTone } from '../../shell/runtime';

/** Presentation only: nothing here is read back as a preference or as state. */

export const LANDING_LABEL: Record<LandingPage, string> = {
  console: 'Console',
  sessions: 'Sessions',
  agents: 'Agents',
  approvals: 'Approvals',
  events: 'Events',
};
export const CURSOR_LABEL: Record<CursorStyle, string> = {
  block: 'Block',
  bar: 'Bar',
  underline: 'Underline',
};
export const TIME_LABEL: Record<TimeFormat, string> = {
  system: 'Follow the system',
  h12: '12-hour',
  h24: '24-hour',
};
export const FILTER_PREFERENCE_LABEL: Record<EventFilterPreference, string> = {
  all: 'All events',
  sessions: 'Sessions',
  runtime: 'Runtime',
  turns: 'Turns',
  approvals: 'Approvals',
  security: 'Security',
};

/** Sizes offered. The bounds are the desktop's; these are only the steps shown. */
export const FONT_SIZES: readonly number[] = Array.from(
  { length: FONT_SIZE.max - FONT_SIZE.min + 1 },
  (_, i) => FONT_SIZE.min + i,
);
const SCROLLBACK_STEPS = [1000, 2000, 5000, 10_000, 20_000, 50_000] as const;
/** The steps offered, and the value in force when it is not one of them. */
export function scrollbackChoices(current: number): readonly number[] {
  return SCROLLBACK_STEPS.includes(current as never)
    ? SCROLLBACK_STEPS
    : [...SCROLLBACK_STEPS, current].sort((a, b) => a - b);
}
export function scrollbackLabel(lines: number): string {
  return `${lines.toLocaleString('en-US')} lines`;
}

export interface SettingsNotice {
  tone: RuntimeTone;
  title: string;
  text: string;
}
/** What is said when the preferences are not simply saved and in force. */
export const STORAGE_NOTICE: Record<StorageState, SettingsNotice | null> = {
  ready: null,
  recovered: {
    tone: 'warn',
    title: 'Your saved preferences could not be read',
    text: 'They were set aside and the defaults are in use. Change a preference to save a new set, or reset to start clean.',
  },
  newer: {
    tone: 'warn',
    title: 'Saved by a newer version of Wyss Console',
    text: 'These preferences were left as they are and the defaults are in use. They cannot be changed from this version until they are reset.',
  },
  unsafe: {
    tone: 'warn',
    title: 'Your saved preferences were not trusted',
    text: 'The file is not private to you, so it was not read and the defaults are in use. Changing a preference replaces it.',
  },
  unavailable: {
    tone: 'error',
    title: 'Preferences cannot be saved here',
    text: 'The place they are kept in is not safely yours, so the defaults are in use and changes are not kept.',
  },
};

export const FAILURE_TEXT: Record<SettingsFailureCode, string> = {
  invalid: 'That value is not allowed.',
  read_only:
    'These preferences were saved by a newer version. Reset them to change them from this version.',
  not_saved: 'The preference could not be saved. Nothing was changed.',
  busy: 'Wyss Console is busy. Try again in a moment.',
  internal: 'The preference could not be saved. Nothing was changed.',
};

export interface Fact {
  term: string;
  value: string;
  tone?: RuntimeTone;
}

/**
 * What the Authority section says: the connection, the Codex runtime, who installed the
 * authority and whether this app can work with it. Closed words only; no path, process,
 * socket or service name, and nothing that is not already shown in the top bar.
 */
export function authorityFacts(
  authority: AuthorityObservation,
  lifecycle: LifecycleObservation,
): Fact[] {
  const link = summarizeAuthority(authority);
  const connected =
    authority.state === 'observed' && authority.view.connection === 'connected';
  const view = lifecycle.state === 'observed' ? lifecycle.view : null;
  const facts: Fact[] = [
    {
      term: 'Authority',
      value: connected
        ? 'Connected'
        : view?.phase === 'failed'
          ? 'Not running'
          : link.label === 'Offline'
            ? 'Not running'
            : link.label,
      tone: link.tone,
    },
  ];
  const agent = summarizeAgent(authority);
  facts.push({
    term: 'Codex runtime',
    value: connected ? agent.label : 'Not available while offline',
    tone: connected ? agent.tone : 'idle',
  });
  facts.push({
    term: 'Installation',
    value:
      view?.origin === 'managed'
        ? 'Managed by Wyss Console'
        : view?.origin === 'external'
          ? 'Started outside Wyss Console'
          : view?.diagnostics.installation === 'current'
            ? 'Installed by Wyss Console'
            : view?.diagnostics.installation === 'absent'
              ? 'Not installed'
              : 'Not known yet',
  });
  const incompatible =
    view?.phase === 'incompatible' ||
    (authority.state === 'observed' &&
      authority.view.reason === 'incompatible');
  facts.push({
    term: 'Compatibility',
    value: incompatible
      ? 'This Wyss authority is not compatible with this version of the app.'
      : connected
        ? 'Compatible'
        : 'Checked when the authority answers',
    tone: incompatible ? 'error' : connected ? 'ok' : 'idle',
  });
  if (authority.state === 'observed' && authority.view.authority) {
    const { service_version: version, protocol_version: protocol } =
      authority.view.authority;
    facts.push({
      term: 'Authority version',
      value: version ?? 'Not reported',
    });
    facts.push({ term: 'Protocol', value: `Version ${protocol}` });
  }
  if (view?.update_pending)
    facts.push({
      term: 'Update',
      value: 'A newer authority is installed the next time it starts',
    });
  return facts;
}

/** `linux-x86_64` as a person would say it. */
export function platformLabel(platform: string): string {
  const [system, architecture] = platform.split('-');
  const name = system === 'linux' ? 'Linux' : system;
  return `${name} (${architecture})`;
}
