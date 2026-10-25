import { afterEach, beforeEach, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import LifecycleCard from '../src/lib/features/terminal/LifecycleCard.svelte';
import { parseHostLifecycle } from '../src/lib/contracts/terminal';

const event = (id: string, outcome: string, current = true) => ({
  id,
  outcome,
  current,
  basis: `basis of ${id}`,
  known_by: 'tested',
});
const facts = (lingering: boolean | null) => ({
  lingering,
  kill_user_processes: false,
  placement: 'user_manager',
  events: [
    event('window_close', 'survives'),
    event('logout_without_lingering', 'ends', !lingering),
    event('logout_with_lingering', 'survives', !!lingering),
    event('reboot_or_power_off', 'ends'),
  ],
});

let component: ReturnType<typeof mount> | undefined;
let calls: { command: string; args: Record<string, unknown> }[];
let current: ReturnType<typeof facts>;
const settle = async () => {
  for (let i = 0; i < 40; i++) await Promise.resolve();
  flushSync();
};
beforeEach(() => {
  calls = [];
  current = facts(false);
  mockIPC((command, args) => {
    calls.push({ command, args: (args as Record<string, unknown>) ?? {} });
    if (command === 'host_persistence') return current;
    if (command === 'host_lingering') {
      const { enable } = (args as { request: { enable: boolean } }).request;
      current = facts(enable);
      return current;
    }
    return null;
  });
});
afterEach(() => {
  if (component) void unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});
const open = async () => {
  component = mount(LifecycleCard, { target: document.body });
  await settle();
};
const button = (text: RegExp) =>
  [...document.querySelectorAll('button')].find((b) =>
    text.test(b.textContent ?? ''),
  );

it('tells what survives what, in plain words, with power-off ending terminals and nothing promised', async () => {
  await open();
  const text = document.body.textContent ?? '';
  expect(text).toMatch(/Closing this window/);
  expect(text).toMatch(/Reboot or power-off/);
  const rows = [...document.querySelectorAll('tbody tr')];
  const outcome = (title: RegExp) =>
    rows.find((r) => title.test(r.textContent ?? ''))?.querySelectorAll('td')[1]
      ?.textContent;
  expect(outcome(/Closing this window/)).toBe('Keeps running');
  expect(outcome(/Reboot or power-off/)).toBe('Ends');
  expect(text).toMatch(/never turns it on by itself/);
});

it('turns lingering on only after a confirmation that says what it runs, and not at all when cancelled', async () => {
  await open();
  button(/Keep terminals after logout/)!.click();
  await settle();
  expect(document.body.textContent).toMatch(/loginctl enable-linger/);
  expect(calls.filter((c) => c.command === 'host_lingering')).toHaveLength(0);
  button(/Cancel/)!.click();
  await settle();
  expect(calls.filter((c) => c.command === 'host_lingering')).toHaveLength(0);
  button(/Keep terminals after logout/)!.click();
  await settle();
  button(/Turn lingering on/)!.click();
  await settle();
  const set = calls.filter((c) => c.command === 'host_lingering');
  expect(set).toHaveLength(1);
  expect(set[0]!.args).toEqual({
    request: { enable: true, confirm: 'enable-lingering' },
  });
  expect(document.body.textContent).toMatch(/Lingering is on/);
  expect(button(/Turn lingering off/)).toBeTruthy();
});

it('offers nothing to change when the system does not say, and survives a host that cannot answer', async () => {
  current = facts(null);
  await open();
  expect(button(/lingering/i)).toBeUndefined();
  expect(document.body.textContent).toMatch(/not something this system lets/);
  if (component) void unmount(component);
  document.body.innerHTML = '';
  mockIPC(() => {
    throw { code: 'unavailable' };
  });
  await open();
  expect(document.body.textContent).toMatch(/could not be read/);
});

it('decodes only an exact lifecycle answer', () => {
  expect(parseHostLifecycle(facts(true)).lingering).toBe(true);
  for (const bad of [
    null,
    {},
    { ...facts(true), placement: 'mars' },
    { ...facts(true), events: [{ id: 'x' }] },
    { ...facts(true), kill_user_processes: 'no' },
    { ...facts(true), events: [event('x', 'maybe')] },
  ])
    expect(() => parseHostLifecycle(bad)).toThrow();
});
