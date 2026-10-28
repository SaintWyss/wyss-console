import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import NativeHarness from './NativeHarness.svelte';
import { NativeCallError } from '../src/lib/desktop/native-client';
import { STOP_UNRECORDED } from '../src/lib/features/native/present';
import { fakeClient, launcherWithM9, session } from './native-harness';

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
const buttons = () =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('button'));
const byLabel = (label: string) =>
  buttons().find((b) => b.textContent?.trim() === label);
const startOpencode = () =>
  buttons().find((b) => b.textContent?.includes('Start OpenCode'));
const text = () => document.body.textContent ?? '';

const runningOpencode = () =>
  session({ agent_id: 'opencode', agent_name: 'OpenCode' });
const stoppedOpencode = () =>
  session({
    agent_id: 'opencode',
    agent_name: 'OpenCode',
    state: 'stopped',
    ended_at: 1_700_000_001,
    terminal_id: null,
    pid: null,
  });
const storeFailure = () =>
  new NativeCallError({ code: 'store_unavailable', reasons: [] });

async function mountWithOpencodeSelected(
  client: ReturnType<typeof fakeClient>,
) {
  component = mount(NativeHarness, {
    target: document.body,
    props: { client },
  });
  await settle();
  document.querySelector<HTMLInputElement>('input[value="opencode"]')!.click();
  await settle();
}

describe('the page after a stop whose M9 record failed', () => {
  it('shows the stopped process, the unrecorded ending, and blocks OpenCode', async () => {
    vi.useFakeTimers();
    let stopped = false;
    const client = fakeClient({
      launcher: vi.fn(async () => launcherWithM9('ready')),
      readiness: vi.fn(async () =>
        launcherWithM9(stopped ? 'storage_unavailable' : 'ready'),
      ),
      sessions: vi.fn(async () => [
        stopped ? stoppedOpencode() : runningOpencode(),
      ]),
      stop: vi.fn(async () => {
        stopped = true;
        throw storeFailure();
      }),
    });
    await mountWithOpencodeSelected(client);
    expect(startOpencode()?.disabled).toBe(false);

    byLabel('Stop')!.click();
    await vi.advanceTimersByTimeAsync(0);
    await settle();

    expect(client.stop).toHaveBeenCalledTimes(1);
    expect(text()).toContain('Stopped');
    expect(text()).toContain(STOP_UNRECORDED);
    expect(text()).toContain('OpenCode and Hermes are blocked');
    expect(text()).not.toContain('could not be read');
    expect(byLabel('Stop')).toBeUndefined();
    expect(startOpencode()?.disabled).toBe(true);
    expect(client.start).not.toHaveBeenCalled();
  });

  it('waits for the store when the launcher cannot be read, and checks again on request', async () => {
    vi.useFakeTimers();
    let failing = false;
    let stopped = false;
    const client = fakeClient({
      launcher: vi.fn(async () => launcherWithM9('ready')),
      readiness: vi.fn(async () => {
        if (failing) throw new Error('the desktop did not answer');
        return launcherWithM9('ready');
      }),
      sessions: vi.fn(async () => [
        stopped ? stoppedOpencode() : runningOpencode(),
      ]),
      stop: vi.fn(async () => {
        stopped = true;
        failing = true;
        throw storeFailure();
      }),
    });
    await mountWithOpencodeSelected(client);

    byLabel('Stop')!.click();
    await vi.advanceTimersByTimeAsync(0);
    await settle();

    expect(text()).toContain('wait until the Console confirms');
    expect(text()).toContain(STOP_UNRECORDED);
    expect(startOpencode()?.disabled).toBe(true);

    failing = false;
    byLabel('Check again')!.click();
    await vi.advanceTimersByTimeAsync(0);
    await settle();

    expect(text()).not.toContain('wait until the Console confirms');
    expect(startOpencode()?.disabled).toBe(false);
    expect(client.stop).toHaveBeenCalledTimes(1);
  });

  it('changes nothing when the stop is recorded', async () => {
    vi.useFakeTimers();
    let stopped = false;
    const client = fakeClient({
      launcher: vi.fn(async () => launcherWithM9('ready')),
      readiness: vi.fn(async () => launcherWithM9('ready')),
      sessions: vi.fn(async () => [
        stopped ? stoppedOpencode() : runningOpencode(),
      ]),
      stop: vi.fn(async () => {
        stopped = true;
      }),
    });
    await mountWithOpencodeSelected(client);

    byLabel('Stop')!.click();
    await vi.advanceTimersByTimeAsync(0);
    await settle();

    expect(text()).toContain('Stopped');
    expect(text()).not.toContain('blocked');
    expect(text()).not.toContain('wait until the Console confirms');
    expect(startOpencode()?.disabled).toBe(false);
    expect(client.stop).toHaveBeenCalledTimes(1);
  });
});
