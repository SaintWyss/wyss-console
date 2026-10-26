import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DEFAULT_PREFERENCES } from '../src/lib/contracts/settings';
import {
  SettingsController,
  type SettingsState,
} from '../src/lib/features/settings/controller';
import { FakeDesktopSettings, withPreferences } from './settings-harness';

const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};
let states: SettingsState[];
function make(desktop: FakeDesktopSettings) {
  states = [];
  const controller = new SettingsController(
    (state) => states.push(state),
    desktop.client(),
  );
  return controller;
}
const last = () => states[states.length - 1]!;
const font = (size: number) => (p: typeof DEFAULT_PREFERENCES) => ({
  ...p,
  terminal: { ...p.terminal, font_size: size },
});

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

it('starts on the defaults, reads once, and shows what it read', async () => {
  const desktop = new FakeDesktopSettings(
    withPreferences((p) => ({
      ...p,
      interface: { ...p.interface, landing_page: 'events' },
    })),
  );
  const controller = make(desktop);
  expect(controller.state()).toMatchObject({
    phase: 'loading',
    view: null,
    preferences: DEFAULT_PREFERENCES,
  });
  controller.start();
  controller.start();
  await settle();
  expect(desktop.calls.map((c) => c.command)).toEqual(['read']);
  expect(last().phase).toBe('ready');
  expect(last().preferences.interface.landing_page).toBe('events');
});

it('does not poll, watch or schedule anything', async () => {
  const desktop = new FakeDesktopSettings();
  const controller = make(desktop);
  controller.start();
  await settle();
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(10 * 60_000);
  expect(desktop.calls).toHaveLength(1);
  controller.change(font(18));
  await settle();
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(10 * 60_000);
  expect(desktop.calls.map((c) => c.command)).toEqual(['read', 'update']);
});

it('shows a change at once and keeps what the desktop confirmed', async () => {
  const desktop = new FakeDesktopSettings();
  const controller = make(desktop);
  controller.start();
  await settle();
  controller.change(font(20));
  expect(last().preferences.terminal.font_size).toBe(20);
  expect(last().saving).toBe(true);
  await settle();
  expect(last().saving).toBe(false);
  expect(last().preferences.terminal.font_size).toBe(20);
  expect(desktop.preferences.terminal.font_size).toBe(20);
  expect(last().failure).toBeNull();
});

it('takes back a change the desktop did not keep, and says why', async () => {
  for (const code of ['not_saved', 'busy', 'internal'] as const) {
    const desktop = new FakeDesktopSettings();
    const controller = make(desktop);
    controller.start();
    await settle();
    desktop.failNext = code;
    controller.change(font(22));
    expect(last().preferences.terminal.font_size).toBe(22);
    await settle();
    expect(last().preferences.terminal.font_size).toBe(16);
    expect(last().failure).toBe(code);
    // The next change clears the notice.
    controller.change(font(18));
    expect(last().failure).toBeNull();
    await settle();
    expect(last().preferences.terminal.font_size).toBe(18);
  }
});

it('sends one write at a time and folds changes made meanwhile into the next', async () => {
  const desktop = new FakeDesktopSettings();
  desktop.hold = true;
  const controller = make(desktop);
  controller.start();
  await settle();
  controller.change(font(11));
  controller.change(font(12));
  controller.change(font(13));
  await settle();
  // Only the first went out; the window already shows the last.
  expect(desktop.calls.filter((c) => c.command === 'update')).toHaveLength(1);
  expect(last().preferences.terminal.font_size).toBe(13);
  desktop.release();
  await settle();
  const updates = desktop.calls.filter((c) => c.command === 'update');
  expect(updates).toHaveLength(2);
  desktop.release();
  await settle();
  expect(desktop.preferences.terminal.font_size).toBe(13);
  expect(last().saving).toBe(false);
  expect(last().preferences.terminal.font_size).toBe(13);
});

it('composes changes to different preferences instead of losing one', async () => {
  const desktop = new FakeDesktopSettings();
  desktop.hold = true;
  const controller = make(desktop);
  controller.start();
  await settle();
  controller.change(font(14));
  controller.change((p) => ({
    ...p,
    interface: { ...p.interface, landing_page: 'sessions' },
  }));
  desktop.release();
  await settle();
  desktop.release();
  await settle();
  expect(desktop.preferences.terminal.font_size).toBe(14);
  expect(desktop.preferences.interface.landing_page).toBe('sessions');
});

it('changes nothing before the first read answers, or while the preferences are a newer version’s', async () => {
  const desktop = new FakeDesktopSettings();
  const controller = make(desktop);
  controller.change(font(20));
  expect(desktop.calls).toEqual([]);
  controller.start();
  await settle();
  const newer = new FakeDesktopSettings(DEFAULT_PREFERENCES, 'newer');
  const other = make(newer);
  other.start();
  await settle();
  other.change(font(20));
  await settle();
  expect(last().failure).toBe('read_only');
  expect(last().preferences.terminal.font_size).toBe(16);
  expect(newer.preferences).toEqual(DEFAULT_PREFERENCES);
});

it('asks before it resets, and a reset that is not confirmed does nothing', async () => {
  const desktop = new FakeDesktopSettings(withPreferences(font(20)));
  const controller = make(desktop);
  controller.start();
  await settle();
  controller.confirmReset();
  await settle();
  expect(desktop.calls.map((c) => c.command)).toEqual(['read']);
  controller.askReset();
  expect(last().confirmingReset).toBe(true);
  controller.cancelReset();
  expect(last().confirmingReset).toBe(false);
  controller.confirmReset();
  await settle();
  expect(desktop.calls.map((c) => c.command)).toEqual(['read']);
  controller.askReset();
  controller.confirmReset();
  await settle();
  expect(desktop.calls.map((c) => c.command)).toEqual(['read', 'reset']);
  expect(last().preferences).toEqual(DEFAULT_PREFERENCES);
  expect(last().confirmingReset).toBe(false);
});

it('never lets a reset overlap a write, and drops a change not yet written', async () => {
  const desktop = new FakeDesktopSettings();
  desktop.hold = true;
  const controller = make(desktop);
  controller.start();
  await settle();
  controller.change(font(11));
  controller.change(font(12));
  controller.askReset();
  controller.confirmReset();
  await settle();
  // The reset waits for the write in flight.
  expect(desktop.calls.map((c) => c.command)).toEqual(['read', 'update']);
  desktop.release();
  await settle();
  expect(desktop.calls.map((c) => c.command)).toEqual([
    'read',
    'update',
    'reset',
  ]);
  desktop.release();
  await settle();
  // Reset was last: the change made before it is not written afterwards.
  expect(desktop.calls.map((c) => c.command)).toEqual([
    'read',
    'update',
    'reset',
  ]);
  expect(desktop.preferences).toEqual(DEFAULT_PREFERENCES);
  expect(last().preferences).toEqual(DEFAULT_PREFERENCES);
  expect(last().saving).toBe(false);
});

it('offers another read after one that failed, and keeps the defaults meanwhile', async () => {
  const desktop = new FakeDesktopSettings(withPreferences(font(20)));
  desktop.failReads = true;
  const controller = make(desktop);
  controller.start();
  await settle();
  expect(last().phase).toBe('failed');
  expect(last().preferences).toEqual(DEFAULT_PREFERENCES);
  controller.change(font(12));
  controller.askReset();
  await settle();
  expect(desktop.calls.map((c) => c.command)).toEqual(['read']);
  desktop.failReads = false;
  controller.retry();
  await settle();
  expect(last().phase).toBe('ready');
  expect(last().preferences.terminal.font_size).toBe(20);
});

it('says nothing after it is disposed', async () => {
  const desktop = new FakeDesktopSettings();
  desktop.hold = true;
  const controller = make(desktop);
  controller.start();
  await settle();
  controller.change(font(13));
  const seen = states.length;
  controller.dispose();
  desktop.release();
  await settle();
  controller.change(font(14));
  controller.askReset();
  expect(states).toHaveLength(seen);
});
