/**
 * SettingsView v1: the desktop's own preferences, as the desktop says they are. Closed
 * words, bounded numbers and flags. There is no text, no path and no map in it: a
 * preference changes how something is shown, never what is allowed.
 *
 * The window checks what it receives and what it sends, and the desktop checks both
 * again: Rust is the judge of every value.
 */
export const LANDING_PAGES = [
  'console',
  'sessions',
  'agents',
  'approvals',
  'events',
] as const;
export type LandingPage = (typeof LANDING_PAGES)[number];

export const CURSOR_STYLES = ['block', 'bar', 'underline'] as const;
export type CursorStyle = (typeof CURSOR_STYLES)[number];

export const TIME_FORMATS = ['system', 'h12', 'h24'] as const;
export type TimeFormat = (typeof TIME_FORMATS)[number];

export const EVENT_FILTER_PREFERENCES = [
  'all',
  'sessions',
  'runtime',
  'turns',
  'approvals',
  'security',
] as const;
export type EventFilterPreference = (typeof EVENT_FILTER_PREFERENCES)[number];

export const FONT_SIZE = { min: 10, max: 24 } as const;
export const SCROLLBACK = { min: 500, max: 50_000 } as const;

export interface Preferences {
  schema_version: 1;
  interface: { landing_page: LandingPage };
  terminal: {
    font_size: number;
    scrollback: number;
    cursor_style: CursorStyle;
    cursor_blink: boolean;
  };
  events: { time_format: TimeFormat; default_filter: EventFilterPreference };
}

/** What the product was before it had preferences. The desktop's defaults are these. */
export const DEFAULT_PREFERENCES: Preferences = {
  schema_version: 1,
  interface: { landing_page: 'console' },
  terminal: {
    font_size: 16,
    scrollback: 5000,
    cursor_style: 'block',
    cursor_blink: false,
  },
  events: { time_format: 'system', default_filter: 'all' },
};

/** How the preferences are kept. */
export const STORAGE_STATES = [
  'ready',
  'recovered',
  'newer',
  'unsafe',
  'unavailable',
] as const;
export type StorageState = (typeof STORAGE_STATES)[number];

export interface About {
  version: string;
  /** A short source revision, when the build knew one. */
  revision: string | null;
  /** Operating system and architecture. */
  platform: string;
}
export interface SettingsView {
  schema_version: 1;
  storage: StorageState;
  preferences: Preferences;
  about: About;
}

export const SETTINGS_FAILURES = [
  'invalid',
  'read_only',
  'not_saved',
  'busy',
  'internal',
] as const;
export type SettingsFailureCode = (typeof SETTINGS_FAILURES)[number];

function invalid(field: string): Error {
  return new Error(`settings: invalid_${field}`);
}
function record(
  value: unknown,
  keys: readonly string[],
  field: string,
): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw invalid(field);
  const names = Object.keys(value);
  if (names.length !== keys.length || !keys.every((k) => names.includes(k)))
    throw invalid(field);
  return value as Record<string, unknown>;
}
function member<T extends string>(
  value: unknown,
  allowed: readonly T[],
  field: string,
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T))
    throw invalid(field);
  return value as T;
}
function bounded(
  value: unknown,
  range: { min: number; max: number },
  field: string,
): number {
  if (
    typeof value !== 'number' ||
    !Number.isSafeInteger(value) ||
    value < range.min ||
    value > range.max
  )
    throw invalid(field);
  return value;
}
function flag(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw invalid(field);
  return value;
}

/** Accepts exactly a preference set, with every value in bounds. Nothing is guessed. */
export function parsePreferences(value: unknown): Preferences {
  const v = record(
    value,
    ['schema_version', 'interface', 'terminal', 'events'],
    'preferences',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  const i = record(v.interface, ['landing_page'], 'interface');
  const t = record(
    v.terminal,
    ['font_size', 'scrollback', 'cursor_style', 'cursor_blink'],
    'terminal',
  );
  const e = record(v.events, ['time_format', 'default_filter'], 'events');
  return {
    schema_version: 1,
    interface: {
      landing_page: member(i.landing_page, LANDING_PAGES, 'landing'),
    },
    terminal: {
      font_size: bounded(t.font_size, FONT_SIZE, 'font_size'),
      scrollback: bounded(t.scrollback, SCROLLBACK, 'scrollback'),
      cursor_style: member(t.cursor_style, CURSOR_STYLES, 'cursor_style'),
      cursor_blink: flag(t.cursor_blink, 'cursor_blink'),
    },
    events: {
      time_format: member(e.time_format, TIME_FORMATS, 'time_format'),
      default_filter: member(
        e.default_filter,
        EVENT_FILTER_PREFERENCES,
        'default_filter',
      ),
    },
  };
}

/** Short printable ASCII, or null: nothing else is rendered as a version. */
function text(value: unknown, pattern: RegExp, field: string): string {
  if (typeof value !== 'string' || !pattern.test(value)) throw invalid(field);
  return value;
}

export function parseSettingsView(value: unknown): SettingsView {
  const v = record(
    value,
    ['schema_version', 'storage', 'preferences', 'about'],
    'view',
  );
  if (v.schema_version !== 1) throw invalid('schema_version');
  const a = record(v.about, ['version', 'revision', 'platform'], 'about');
  return {
    schema_version: 1,
    storage: member(v.storage, STORAGE_STATES, 'storage'),
    preferences: parsePreferences(v.preferences),
    about: {
      version: text(a.version, /^[0-9A-Za-z][0-9A-Za-z.+-]{0,31}$/, 'version'),
      revision:
        a.revision === null
          ? null
          : text(a.revision, /^[0-9a-f]{7,40}$/, 'revision'),
      platform: text(
        a.platform,
        /^[a-z0-9_]{1,16}-[a-z0-9_]{1,16}$/,
        'platform',
      ),
    },
  };
}

/** The code of a failed settings call, or `internal` for anything unexpected. */
export function parseSettingsFailure(value: unknown): SettingsFailureCode {
  if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
    const code = (value as { code?: unknown }).code;
    if (
      typeof code === 'string' &&
      (SETTINGS_FAILURES as readonly string[]).includes(code)
    )
      return code as SettingsFailureCode;
  }
  return 'internal';
}
