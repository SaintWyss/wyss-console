import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { NativeController } from '../src/lib/features/native/controller';
import { NativeCallError } from '../src/lib/desktop/native-client';
import { START_UNRECORDED } from '../src/lib/features/native/present';
import type {
  Launcher,
  NativeSession,
  Profile,
} from '../src/lib/contracts/native';
import {
  fakeClient,
  launcherWithM9,
  profiles,
  reply,
  session,
} from './native-harness';

const controllers: NativeController[] = [];
const own = (controller: NativeController) => {
  controllers.push(controller);
  return controller;
};
const settle = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const storage = () =>
  new NativeCallError({ code: 'store_unavailable', reasons: [] });
const stopped = () =>
  session({
    agent_id: 'opencode',
    state: 'stopped',
    ended_at: 1_700_000_001,
    terminal_id: null,
    pid: null,
  });

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  controllers.splice(0).forEach((c) => c.dispose());
  vi.useRealTimers();
});

it('detects degradation within one visible interval when another window ends and forgets a session', async () => {
  let degraded = false;
  let records: NativeSession[] = [];
  let effects = 0;
  const client = fakeClient({
    launcher: vi.fn(async () =>
      launcherWithM9(degraded ? 'storage_unavailable' : 'ready'),
    ),
    sessions: vi.fn(async () => records.map((s) => ({ ...s }))),
    start: vi.fn(async () => {
      if (degraded) throw storage();
      effects++;
      const result = reply({ agent_id: 'opencode' });
      records = [result.session];
      return result;
    }),
    stop: vi.fn(async () => {
      records = [stopped()];
      degraded = true;
      throw storage();
    }),
    forget: vi.fn(async () => {
      records = [];
    }),
  });
  const observer = own(
    new NativeController(() => undefined, undefined, client),
  );
  const actor = own(new NativeController(() => undefined, undefined, client));
  observer.setVisible(true);
  actor.setVisible(true);
  await settle();
  await actor.startAgent('opencode', '/home/u', null);
  await settle();
  await actor.stop('a'.repeat(32));
  await settle();
  await actor.forget('a'.repeat(32));
  await settle();
  await vi.advanceTimersByTimeAsync(1999);
  expect(observer.state().sessions).toEqual([]);
  await vi.advanceTimersByTimeAsync(1);
  expect(observer.state().launcher?.m9_native_status).toBe(
    'storage_unavailable',
  );
  for (const agent of ['opencode', 'hermes'] as const)
    expect(await observer.startAgent(agent, '/home/u', null)).toBe(false);
  expect(effects).toBe(1);
  expect(client.start).toHaveBeenCalledTimes(1);
});

it('keeps one heartbeat and one background request per resource when both reads stall', async () => {
  const client = fakeClient({
    launcher: vi.fn(async () => launcherWithM9('ready')),
  });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  const launcher = deferred<Launcher>();
  const sessions = deferred<NativeSession[]>();
  vi.mocked(client.launcher).mockImplementation(() => launcher.promise);
  vi.mocked(client.sessions).mockImplementation(() => sessions.promise);
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().m9Blocked).toBe(false);
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().m9Uncertain).toBe(true);
  expect(controller.state().checkingLauncher).toBe(false); // a manual check may supersede the stalled request
  await vi.advanceTimersByTimeAsync(56_000);
  expect(client.launcher).toHaveBeenCalledTimes(2);
  expect(client.sessions).toHaveBeenCalledTimes(2);
  expect(vi.getTimerCount()).toBe(1);
  vi.mocked(client.launcher).mockResolvedValue(
    launcherWithM9('storage_unavailable'),
  );
  launcher.resolve(launcherWithM9('ready'));
  await settle();
  expect(controller.state().m9Blocked).toBe(true);
  expect(controller.state().launcher?.m9_native_status).toBe(
    'storage_unavailable',
  );
  expect(client.launcher).toHaveBeenCalledTimes(3);
  controller.dispose();
  expect(vi.getTimerCount()).toBe(0);
  sessions.resolve([]);
  await settle();
  expect(client.sessions).toHaveBeenCalledTimes(2);
});

it('recovers from periodic read failures without an application restart or a process retry', async () => {
  let failing = false;
  const client = fakeClient({
    launcher: vi.fn(async () => {
      if (failing) throw new Error('unavailable');
      return launcherWithM9('ready');
    }),
  });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  failing = true;
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().m9Blocked).toBe(true);
  failing = false;
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().m9Blocked).toBe(false);
  expect(client.start).not.toHaveBeenCalled();
  expect(client.stop).not.toHaveBeenCalled();
});

it('invalidates a read before the mutation settles and reconciles a possible start effect once', async () => {
  const old = deferred<Launcher>();
  const starting = deferred<ReturnType<typeof reply>>();
  let reads = 0;
  const effect = session({ id: 'b'.repeat(32), agent_id: 'opencode' });
  let records: NativeSession[] = [];
  const client = fakeClient({
    launcher: vi.fn(() =>
      ++reads === 2
        ? old.promise
        : Promise.resolve(
            launcherWithM9(reads > 2 ? 'storage_unavailable' : 'ready'),
          ),
    ),
    sessions: vi.fn(async () => records),
    start: vi.fn(() => starting.promise),
  });
  const adopted = vi.fn();
  const controller = own(
    new NativeController(() => undefined, adopted, client),
  );
  controller.setVisible(true);
  await settle();
  const reading = controller.refreshLauncher();
  const operation = controller.startAgent('claude_code', '/home/u', null);
  old.resolve(launcherWithM9('ready'));
  await reading;
  expect(controller.state().m9Blocked).toBe(true);
  records = [effect];
  starting.reject(storage());
  expect(await operation).toBe(false);
  await settle();
  expect(controller.state().sessions).toEqual([effect]);
  expect(controller.state().failure?.reasons).toContain(START_UNRECORDED);
  expect(controller.state().m9Blocked).toBe(true);
  expect(client.start).toHaveBeenCalledTimes(1);
  expect(adopted).not.toHaveBeenCalled();
});

it('discards a pre-mutation sessions answer and then publishes the fresh possible effect', async () => {
  const old = deferred<NativeSession[]>();
  let reads = 0;
  const existing = session();
  const effect = session({ id: 'b'.repeat(32) });
  const client = fakeClient({
    sessions: vi.fn(() =>
      ++reads === 2
        ? old.promise
        : Promise.resolve(reads > 2 ? [existing, effect] : [existing]),
    ),
    start: vi.fn(async () => {
      throw storage();
    }),
  });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  controller.refreshSessions();
  await controller.startAgent('claude_code', '/home/u', null);
  await settle();
  old.resolve([]);
  await settle();
  expect(controller.state().sessions).toEqual([existing, effect]);
  expect(client.sessions).toHaveBeenCalledTimes(3);
});

it.each(['start', 'restart'] as const)(
  'preserves the original %s error when both reconciliation reads fail',
  async (operation) => {
    let failed = false;
    const original = new NativeCallError({
      code: 'spawn_failed',
      reasons: ['original reason'],
    });
    const client = fakeClient({
      launcher: vi.fn(async () => {
        if (failed) throw new Error('secondary');
        return launcherWithM9('ready');
      }),
      sessions: vi.fn(async () => {
        if (failed) throw new Error('secondary');
        return [session()];
      }),
      [operation]: vi.fn(async () => {
        failed = true;
        throw original;
      }),
    });
    const controller = own(
      new NativeController(() => undefined, undefined, client),
    );
    controller.setVisible(true);
    await settle();
    const before = vi.mocked(client.sessions).mock.calls.length;
    if (operation === 'start')
      expect(await controller.startAgent('claude_code', '/home/u', null)).toBe(
        false,
      );
    else expect(await controller.restart('a'.repeat(32))).toBe(false);
    await settle();
    expect(controller.state().failure).toEqual(original.failure);
    expect(controller.state().m9Blocked).toBe(true);
    expect(client.sessions).toHaveBeenCalledTimes(before + 1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(client[operation]).toHaveBeenCalledTimes(1);
  },
);

it('waits for concurrent mutations before confirming readiness and never repeats either Stop', async () => {
  const a = deferred<void>();
  const b = deferred<void>();
  let stops = 0;
  const client = fakeClient({
    launcher: vi.fn(async () =>
      launcherWithM9(stops ? 'storage_unavailable' : 'ready'),
    ),
    stop: vi.fn(() => (++stops === 1 ? a.promise : b.promise)),
  });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  const first = controller.stop('a'.repeat(32));
  const second = controller.stop('b'.repeat(32));
  await controller.refreshLauncher();
  a.resolve();
  await first;
  await settle();
  expect(client.launcher).toHaveBeenCalledTimes(1);
  expect(controller.state().m9Blocked).toBe(true);
  b.reject(storage());
  await second;
  await settle();
  expect(client.launcher).toHaveBeenCalledTimes(2);
  expect(client.stop).toHaveBeenCalledTimes(2);
});

it('clears the visible heartbeat on hide, verifies on show, and creates no duplicate timer', async () => {
  const client = fakeClient({
    launcher: vi.fn(async () => launcherWithM9('ready')),
  });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  expect(vi.getTimerCount()).toBe(1);
  controller.setVisible(false);
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(20_000);
  expect(client.launcher).toHaveBeenCalledTimes(1);
  const pending = deferred<Launcher>();
  vi.mocked(client.launcher).mockReturnValue(pending.promise);
  controller.setVisible(true);
  controller.setVisible(true);
  expect(controller.state().m9Blocked).toBe(true);
  expect(vi.getTimerCount()).toBe(1);
  expect(client.launcher).toHaveBeenCalledTimes(2);
  pending.resolve(launcherWithM9('ready'));
  await settle();
  expect(controller.state().m9Blocked).toBe(false);
  controller.dispose();
  expect(vi.getTimerCount()).toBe(0);
});

it('ignores late initial reads and sends no work after dispose', async () => {
  const launcher = deferred<Launcher>();
  const p = deferred<Profile[]>();
  const s = deferred<NativeSession[]>();
  const client = fakeClient({
    launcher: vi.fn(() => launcher.promise),
    profiles: vi.fn(() => p.promise),
    sessions: vi.fn(() => s.promise),
  });
  const publish = vi.fn();
  const controller = own(new NativeController(publish, undefined, client));
  controller.setVisible(true);
  controller.dispose();
  const count = publish.mock.calls.length;
  launcher.resolve(launcherWithM9('ready'));
  p.resolve(profiles());
  s.resolve([session()]);
  await settle();
  await controller.refreshLauncher();
  controller.refreshSessions();
  await controller.stop('a'.repeat(32));
  expect(await controller.startAgent('claude_code', '/home/u', null)).toBe(
    false,
  );
  expect(publish).toHaveBeenCalledTimes(count);
  expect(controller.state().sessions).toEqual([]);
  expect(client.launcher).toHaveBeenCalledTimes(1);
  expect(client.stop).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

it('does not adopt a late Start reply after the controller is disposed', async () => {
  const pending = deferred<ReturnType<typeof reply>>();
  const client = fakeClient({ start: vi.fn(() => pending.promise) });
  const adopt = vi.fn();
  const publish = vi.fn();
  const controller = own(new NativeController(publish, adopt, client));
  controller.setVisible(true);
  await settle();
  const start = controller.startAgent('claude_code', '/home/u', null);
  controller.dispose();
  const emitted = publish.mock.calls.length;
  pending.resolve(reply());
  expect(await start).toBe(false);
  await settle();
  expect(adopt).not.toHaveBeenCalled();
  expect(publish).toHaveBeenCalledTimes(emitted);
  expect(client.start).toHaveBeenCalledTimes(1);
  expect(client.launcher).toHaveBeenCalledTimes(1);
  expect(vi.getTimerCount()).toBe(0);
});

it('reloads profiles and sessions after a valid retry of an initially failed launcher', async () => {
  let failing = true;
  const client = fakeClient({
    launcher: vi.fn(async () => {
      if (failing) throw new Error('down');
      return launcherWithM9('ready');
    }),
  });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  expect(controller.state().phase).toBe('failed');
  failing = false;
  await controller.refreshLauncher();
  await settle();
  expect(controller.state().phase).toBe('ready');
  expect(controller.state().m9Blocked).toBe(false);
  expect(controller.state().profiles).toEqual(profiles());
  expect(client.start).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(1);
});

it('keeps the existing hidden running-session poll without a hidden readiness heartbeat', async () => {
  const client = fakeClient({ sessions: vi.fn(async () => [session()]) });
  const controller = own(
    new NativeController(() => undefined, undefined, client),
  );
  controller.setVisible(true);
  await settle();
  controller.setVisible(false);
  await vi.advanceTimersByTimeAsync(20_000);
  expect(client.launcher).toHaveBeenCalledTimes(1);
  expect(client.sessions).toHaveBeenCalledTimes(6);
  expect(client.sessions).toHaveBeenLastCalledWith(false);
  expect(vi.getTimerCount()).toBe(1);
  controller.dispose();
  expect(vi.getTimerCount()).toBe(0);
});
