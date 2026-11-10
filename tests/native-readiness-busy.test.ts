import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { NativeController } from '../src/lib/features/native/controller';
import { NativeCallError } from '../src/lib/desktop/native-client';
import { fakeClient, launcherWithM9 } from './native-harness';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
const settle = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
};

it('a successful heartbeat clears a readiness_busy failure left by one colliding read', async () => {
  let calls = 0;
  const client = fakeClient({
    readiness: vi.fn(async () => {
      calls++;
      // Only the third read collides with the store lock.
      if (calls === 3)
        throw new NativeCallError({ code: 'readiness_busy', reasons: [] });
      return launcherWithM9('ready');
    }),
  });
  const controller = new NativeController(() => undefined, undefined, client);
  controller.setVisible(true);
  await settle();
  const seen: (string | null)[] = [];
  for (let i = 0; i < 6; i++) {
    await vi.advanceTimersByTimeAsync(2000);
    await settle();
    seen.push(controller.state().failure?.code ?? null);
  }
  expect(calls).toBeGreaterThan(4);
  expect(seen).toContain('readiness_busy');
  expect(controller.state().m9Uncertain).toBe(false);
  expect(controller.state().failure).toBeNull();
  controller.dispose();
});
