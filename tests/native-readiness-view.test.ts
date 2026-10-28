import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import NativeHarness from './NativeHarness.svelte';
import type { Launcher } from '../src/lib/contracts/native';
import { fakeClient, launcherWithM9 } from './native-harness';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  vi.useRealTimers();
});
const settle = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
  flushSync();
};
const button = (label: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  );
const both = (status: Launcher['m9_native_status']) => {
  const value = launcherWithM9(status);
  value.agents.push({
    ...value.agents[1]!,
    agent_id: 'hermes',
    name: 'Hermes Agent (Native)',
    executable: 'hermes',
  });
  return value;
};

it('disables both M9 starts when polling discovers degradation without any session change', async () => {
  vi.useFakeTimers();
  let degraded = false;
  const client = fakeClient({
    launcher: vi.fn(async () =>
      both(degraded ? 'storage_unavailable' : 'ready'),
    ),
  });
  component = mount(NativeHarness, {
    target: document.body,
    props: { client },
  });
  await settle();
  degraded = true;
  await vi.advanceTimersByTimeAsync(20);
  await settle();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    'OpenCode and Hermes are blocked',
  );
  for (const [id, label] of [
    ['opencode', 'OpenCode'],
    ['hermes', 'Hermes Agent (Native)'],
  ]) {
    document.querySelector<HTMLInputElement>(`input[value="${id}"]`)!.click();
    await settle();
    expect(button(`Start ${label}`)?.disabled).toBe(true);
  }
  document
    .querySelector<HTMLInputElement>('input[value="claude_code"]')!
    .click();
  await settle();
  expect(button('Start Claude Code')?.disabled).toBe(false);
  expect(client.start).not.toHaveBeenCalled();
  expect(client.stop).not.toHaveBeenCalled();
});

it('coalesces polling during Check again and restores M9 only after a current valid answer', async () => {
  vi.useFakeTimers();
  let failing = false;
  let pending: Promise<Launcher> | null = null;
  let resolve!: (launcher: Launcher) => void;
  const client = fakeClient({
    launcher: vi.fn(async () => {
      if (pending) return pending;
      if (failing) throw new Error('read failed');
      return both('ready');
    }),
  });
  component = mount(NativeHarness, {
    target: document.body,
    props: { client },
  });
  await settle();
  document.querySelector<HTMLInputElement>('input[value="opencode"]')!.click();
  await settle();
  failing = true;
  await vi.advanceTimersByTimeAsync(20);
  await settle();
  expect(button('Start OpenCode')?.disabled).toBe(true);
  expect(document.querySelector('[role="status"]')?.textContent).toContain(
    'wait until the Console confirms',
  );
  pending = new Promise<Launcher>((yes) => {
    resolve = yes;
  });
  const before = vi.mocked(client.launcher).mock.calls.length;
  button('Check again')!.click();
  button('Check again')!.click();
  await settle();
  expect(button('Check again')?.disabled).toBe(true);
  expect(client.launcher).toHaveBeenCalledTimes(before + 1);
  await vi.advanceTimersByTimeAsync(20);
  await settle();
  expect(client.launcher).toHaveBeenCalledTimes(before + 1);
  expect(button('Check again')?.disabled).toBe(false); // a stalled read may be superseded explicitly
  expect(button('Start OpenCode')?.disabled).toBe(true);
  pending = null;
  failing = false;
  resolve(both('ready'));
  await settle();
  expect(button('Start OpenCode')?.disabled).toBe(false);
  expect(document.body.textContent).not.toContain(
    'wait until the Console confirms',
  );
  expect(client.start).not.toHaveBeenCalled();
});
