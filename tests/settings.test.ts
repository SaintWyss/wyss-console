import { afterEach, expect, it } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  CURSOR_STYLES,
  APPEARANCE_THEMES,
  DEFAULT_PREFERENCES,
  EVENT_FILTER_PREFERENCES,
  LANDING_PAGES,
  STORAGE_STATES,
  TIME_FORMATS,
  parsePreferences,
  parseSettingsFailure,
  parseSettingsView,
} from '../src/lib/contracts/settings';
import {
  SettingsCallError,
  readSettings,
  resetSettings,
  updateSettings,
} from '../src/lib/desktop/settings-client';
import { settingsView, withPreferences } from './settings-harness';

afterEach(() => clearMocks());

/** Every key path of a value, the way the Rust test spells its vocabulary. */
function keys(value: unknown, prefix = ''): string[] {
  if (typeof value !== 'object' || value === null) return [];
  return Object.entries(value).flatMap(([key, inner]) => {
    const name = prefix ? `${prefix}.${key}` : key;
    return [name, ...keys(inner, name)];
  });
}

it('has the defaults the desktop has: the product as it was before preferences', () => {
  expect(DEFAULT_PREFERENCES).toEqual({
    schema_version: 2,
    interface: { landing_page: 'console', theme: 'dark' },
    terminal: {
      font_size: 16,
      scrollback: 5000,
      cursor_style: 'block',
      cursor_blink: false,
    },
    events: { time_format: 'system', default_filter: 'all' },
  });
  expect(parsePreferences(DEFAULT_PREFERENCES)).toEqual(DEFAULT_PREFERENCES);
});

it('names no setting that could decide anything', () => {
  // Spelled out on purpose: a new key is a decision to be made here, in the open. The
  // same list stands in the desktop's own test.
  expect(keys(DEFAULT_PREFERENCES).sort()).toEqual(
    [
      'events',
      'events.default_filter',
      'events.time_format',
      'interface',
      'interface.landing_page',
      'interface.theme',
      'schema_version',
      'terminal',
      'terminal.cursor_blink',
      'terminal.cursor_style',
      'terminal.font_size',
      'terminal.scrollback',
    ].sort(),
  );
  // Inference stays the default: no landing page or mode is a way to Local Tools, and
  // Settings is not a place the window can open on.
  expect(LANDING_PAGES).toEqual([
    'console',
    'sessions',
    'agents',
    'approvals',
    'events',
  ]);
  // Every value is a closed word, a bounded number or a flag.
  for (const words of [
    LANDING_PAGES,
    APPEARANCE_THEMES,
    CURSOR_STYLES,
    TIME_FORMATS,
    EVENT_FILTER_PREFERENCES,
    STORAGE_STATES,
  ])
    for (const word of words) expect(word).toMatch(/^[a-z0-9_]{1,16}$/);
});

it('accepts only the three reviewed appearance themes', () => {
  for (const theme of APPEARANCE_THEMES)
    expect(
      parsePreferences(
        withPreferences((p) => ({
          ...p,
          interface: { ...p.interface, theme },
        })),
      ).interface.theme,
    ).toBe(theme);
  for (const theme of ['system', 'contrast', 'Dark', '']) {
    expect(() =>
      parsePreferences(
        withPreferences((p) => ({
          ...p,
          interface: { ...p.interface, theme } as typeof p.interface,
        })),
      ),
    ).toThrow('settings: invalid_theme');
  }
});

it('accepts a set at each bound, and nothing beyond', () => {
  for (const [font, lines] of [
    [10, 500],
    [24, 50_000],
    [16, 5000],
  ] as const)
    expect(
      parsePreferences(
        withPreferences((p) => ({
          ...p,
          terminal: { ...p.terminal, font_size: font, scrollback: lines },
        })),
      ).terminal,
    ).toMatchObject({ font_size: font, scrollback: lines });
  for (const [font, lines] of [
    [9, 5000],
    [25, 5000],
    [16, 499],
    [16, 50_001],
    [16.5, 5000],
    [16, 5000.5],
    [-1, 5000],
    [16, Number.NaN],
    [16, Number.POSITIVE_INFINITY],
    [2 ** 53, 5000],
  ] as const)
    expect(() =>
      parsePreferences(
        withPreferences((p) => ({
          ...p,
          terminal: { ...p.terminal, font_size: font, scrollback: lines },
        })),
      ),
    ).toThrow(/invalid_/);
});

it('refuses a key it does not know, at any level, and a missing one', () => {
  const base = structuredClone(DEFAULT_PREFERENCES) as unknown as Record<
    string,
    Record<string, unknown>
  >;
  for (const [section, key, value] of [
    ['top', 'auto_approve', true],
    ['top', 'default_mode', 'local_tools'],
    ['top', 'workspace', '/'],
    ['top', 'environment', { LD_PRELOAD: 'x' }],
    ['interface', 'default_mode', 'local_tools'],
    ['terminal', 'shell', '/bin/sh'],
    ['terminal', 'cwd', '/'],
    ['events', 'retention_days', 1],
    ['events', 'audit', false],
  ] as const) {
    const copy = structuredClone(base);
    if (section === 'top') (copy as Record<string, unknown>)[key] = value;
    else copy[section]![key] = value;
    expect(() => parsePreferences(copy), `${section}.${key}`).toThrow();
  }
  for (const section of ['interface', 'terminal', 'events', 'schema_version']) {
    const copy = structuredClone(base) as Record<string, unknown>;
    delete copy[section];
    expect(() => parsePreferences(copy), section).toThrow();
  }
  for (const wrong of [null, undefined, 7, 'x', [], [DEFAULT_PREFERENCES]])
    expect(() => parsePreferences(wrong)).toThrow();
});

it('refuses a word that is not one of the closed set', () => {
  for (const [section, key, value] of [
    ['interface', 'landing_page', 'settings'],
    ['interface', 'landing_page', 'Console'],
    ['interface', 'landing_page', '/etc'],
    ['terminal', 'cursor_style', 'hidden'],
    ['terminal', 'cursor_blink', 'yes'],
    ['terminal', 'cursor_blink', 1],
    ['events', 'time_format', 'iso'],
    ['events', 'default_filter', 'everything'],
  ] as const) {
    const copy = structuredClone(DEFAULT_PREFERENCES) as unknown as Record<
      string,
      Record<string, unknown>
    >;
    copy[section]![key] = value;
    expect(() => parsePreferences(copy), `${section}.${key}`).toThrow();
  }
  expect(() =>
    parsePreferences({ ...DEFAULT_PREFERENCES, schema_version: 1 }),
  ).toThrow();
});

it('parses exactly the view, and nothing that contradicts it', () => {
  expect(parseSettingsView(settingsView())).toEqual(settingsView());
  for (const storage of STORAGE_STATES)
    expect(parseSettingsView(settingsView({ storage })).storage).toBe(storage);
  const bad: unknown[] = [
    { ...settingsView(), storage: 'fine' },
    { ...settingsView(), schema_version: 2 },
    { ...settingsView(), extra: 1 },
    { ...settingsView(), about: { ...settingsView().about, version: '' } },
    {
      ...settingsView(),
      about: { ...settingsView().about, version: '1'.repeat(40) },
    },
    { ...settingsView(), about: { ...settingsView().about, revision: 'XYZ' } },
    {
      ...settingsView(),
      about: { ...settingsView().about, revision: '/home/someone' },
    },
    {
      ...settingsView(),
      about: { ...settingsView().about, platform: '/usr/bin/sh' },
    },
    {
      ...settingsView(),
      about: { ...settingsView().about, platform: 'linux' },
    },
    { ...settingsView(), about: { version: '1', platform: 'linux-x86_64' } },
    { ...settingsView(), preferences: { ...DEFAULT_PREFERENCES, extra: 1 } },
    null,
    [],
    'view',
  ];
  for (const value of bad) expect(() => parseSettingsView(value)).toThrow();
  // A source revision is optional; a missing one is null, never an empty string.
  expect(
    parseSettingsView(
      settingsView({
        about: { version: '1.0.0', revision: null, platform: 'linux-aarch64' },
      }),
    ).about.revision,
  ).toBeNull();
});

it('reads a failure code only from the closed set', () => {
  expect(parseSettingsFailure({ code: 'read_only' })).toBe('read_only');
  for (const wrong of [
    { code: 'boom' },
    { code: 7 },
    'read_only',
    null,
    undefined,
    [],
    new Error('raw rust error at /home/x'),
  ])
    expect(parseSettingsFailure(wrong)).toBe('internal');
});

it('calls three commands, each with the request that names nothing else', async () => {
  const calls: { command: string; payload: unknown }[] = [];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    return settingsView();
  });
  await readSettings();
  await resetSettings();
  const next = withPreferences((p) => ({
    ...p,
    terminal: { ...p.terminal, font_size: 20 },
  }));
  await updateSettings(next);
  expect(calls).toEqual([
    { command: 'settings_read', payload: { request: {} } },
    { command: 'settings_reset', payload: { request: {} } },
    { command: 'settings_update', payload: { request: { preferences: next } } },
  ]);
});

it('never sends a set it can see is invalid', async () => {
  const calls: string[] = [];
  mockIPC((command) => {
    calls.push(command);
    return settingsView();
  });
  const bad = withPreferences((p) => ({
    ...p,
    terminal: { ...p.terminal, font_size: 99 },
  }));
  await expect(updateSettings(bad)).rejects.toMatchObject({ code: 'invalid' });
  await expect(
    updateSettings({ ...DEFAULT_PREFERENCES, extra: 1 } as never),
  ).rejects.toBeInstanceOf(SettingsCallError);
  expect(calls).toEqual([]);
});

it('turns a refusal into its code, and anything else into internal', async () => {
  for (const [answer, code] of [
    [{ code: 'read_only' }, 'read_only'],
    [{ code: 'not_saved' }, 'not_saved'],
    [{ code: 'busy' }, 'busy'],
    ['Error: missing field `schema_version` at line 1', 'internal'],
    [{ code: 'surprise' }, 'internal'],
  ] as const) {
    mockIPC(() => Promise.reject(answer));
    await expect(readSettings()).rejects.toMatchObject({ code });
  }
});

it('believes nothing that is not a view', async () => {
  for (const answer of [
    {},
    null,
    { ...settingsView(), storage: 'ready ' },
    {
      ...settingsView(),
      preferences: { ...DEFAULT_PREFERENCES, terminal: {} },
    },
    settingsView({
      about: { version: '1', revision: null, platform: '/proc/self/environ' },
    }),
  ]) {
    mockIPC(() => answer);
    await expect(readSettings()).rejects.toMatchObject({ code: 'internal' });
    await expect(resetSettings()).rejects.toMatchObject({ code: 'internal' });
  }
});
