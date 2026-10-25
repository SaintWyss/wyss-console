import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  M3Controller,
  type M3ViewState,
} from '../src/lib/features/m3/controller';
import type { M3Client } from '../src/lib/desktop/m3-client';
import { parseM3State, type Ack } from '../src/lib/contracts/m3';
import inactive from './fixtures/m3/state-inactive.json';
import held from './fixtures/m3/state-fixture-tools-held.json';

const ok: Ack = { ok: true, error: null };
const state = (raw: unknown) => parseM3State(JSON.parse(JSON.stringify(raw)));

function client(overrides: Partial<M3Client> = {}) {
  const calls: string[] = [];
  const c: M3Client = {
    state: vi.fn(async () => state(inactive)),
    fixtureStart: vi.fn(async (p) => (calls.push(`start ${p}`), ok)),
    fixtureStop: vi.fn(async () => (calls.push('stop'), ok)),
    fixturePrompt: vi.fn(async (t) => (calls.push(`prompt ${t}`), ok)),
    fixtureInterrupt: vi.fn(async () => (calls.push('interrupt'), ok)),
    fixtureDecide: vi.fn(async (i, d) => (calls.push(`decide ${i} ${d}`), ok)),
    ...overrides,
  };
  return { c, calls };
}
const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it('reads nothing while hidden and reads at once when shown', async () => {
  const { c } = client();
  const seen: M3ViewState[] = [];
  const ctl = new M3Controller(c, (s) => seen.push(s));
  await vi.advanceTimersByTimeAsync(5000);
  expect(c.state).not.toHaveBeenCalled();
  ctl.setVisible(true);
  await settle();
  expect(c.state).toHaveBeenCalledTimes(1);
  expect(ctl.state().status).toBe('ready');
  ctl.setVisible(false);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(c.state).toHaveBeenCalledTimes(1);
  ctl.dispose();
});

it('polls slowly at rest and quickly while a fixture runtime is live', async () => {
  const { c } = client({ state: vi.fn(async () => state(held)) });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  await settle();
  const first = (c.state as ReturnType<typeof vi.fn>).mock.calls.length;
  await vi.advanceTimersByTimeAsync(1000);
  // The held fixture is "ready" with a turn running: the fast cadence.
  expect(
    (c.state as ReturnType<typeof vi.fn>).mock.calls.length - first,
  ).toBeGreaterThanOrEqual(3);
  ctl.dispose();
  const after = (c.state as ReturnType<typeof vi.fn>).mock.calls.length;
  await vi.advanceTimersByTimeAsync(5000);
  expect((c.state as ReturnType<typeof vi.fn>).mock.calls.length).toBe(after);
});

it('shows a failure when nothing was ever read, and marks what it kept as stale after', async () => {
  let fail = true;
  const { c } = client({
    state: vi.fn(async () => {
      if (fail) throw new Error('boom');
      return state(inactive);
    }),
  });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  await settle();
  expect(ctl.state()).toMatchObject({
    status: 'failed',
    state: null,
    stale: false,
  });
  fail = false;
  await vi.advanceTimersByTimeAsync(2000);
  expect(ctl.state()).toMatchObject({
    status: 'ready',
    stale: false,
    failure: null,
  });
  fail = true;
  await vi.advanceTimersByTimeAsync(2000);
  expect(ctl.state().stale).toBe(true);
  expect(ctl.state().state).not.toBeNull();
  ctl.dispose();
});

it('drops an older answer that arrives after a newer one', async () => {
  const resolvers: ((s: ReturnType<typeof state>) => void)[] = [];
  const { c } = client({
    state: vi.fn(
      () =>
        new Promise<ReturnType<typeof state>>((r) => {
          resolvers.push(r);
        }),
    ),
  });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  void ctl.refresh();
  await settle();
  const older = state(inactive);
  const newer = state(held);
  resolvers[1]!(newer);
  await settle();
  resolvers[0]!(older);
  await settle();
  expect(ctl.state().state?.fixture?.phase).toBe('ready');
  ctl.dispose();
});

it('sends an action once, reports a refusal, and reads again without repeating it', async () => {
  const { c, calls } = client({
    fixtureDecide: vi.fn(async () => ({ ok: false, error: 'not_pending' })),
  });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  await settle();
  await ctl.fixtureDecide('claude-act-1', 'accept');
  expect(c.fixtureDecide).toHaveBeenCalledTimes(1);
  expect(ctl.state().refused).toBe('not_pending');
  expect(ctl.state().busy).toBe(false);
  expect(calls).toEqual([]);
  await vi.advanceTimersByTimeAsync(5000);
  expect(c.fixtureDecide).toHaveBeenCalledTimes(1);
  ctl.dispose();
});

it('refuses a second action while one is in flight', async () => {
  let release!: () => void;
  const { c } = client({
    fixturePrompt: vi.fn(
      () => new Promise<Ack>((r) => (release = () => r(ok))),
    ),
  });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  await settle();
  const first = ctl.fixturePrompt('one');
  await settle();
  expect(ctl.state().busy).toBe(true);
  await ctl.fixturePrompt('two');
  expect(c.fixturePrompt).toHaveBeenCalledTimes(1);
  release();
  await first;
  expect(ctl.state().busy).toBe(false);
  ctl.dispose();
});

it('reports an unreachable service as such and never throws', async () => {
  const { c } = client({
    fixtureStart: vi.fn(async () => {
      throw new Error('ipc');
    }),
  });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  await settle();
  await expect(
    ctl.fixtureStart('claude_inference_v1'),
  ).resolves.toBeUndefined();
  expect(ctl.state().refused).toBe('unreachable');
  ctl.dispose();
});

it('does nothing after it was disposed', async () => {
  const { c } = client();
  const ctl = new M3Controller(c, () => undefined);
  ctl.dispose();
  ctl.setVisible(true);
  await ctl.refresh();
  await ctl.fixtureStop();
  expect(c.state).not.toHaveBeenCalled();
  expect(c.fixtureStop).not.toHaveBeenCalled();
});

it('checks a Claude installation under verification at the fast cadence', async () => {
  const verifying = JSON.parse(JSON.stringify(inactive));
  verifying.claude_install = { state: 'verifying' };
  const { c } = client({ state: vi.fn(async () => state(verifying)) });
  const ctl = new M3Controller(c, () => undefined);
  ctl.setVisible(true);
  await settle();
  const before = (c.state as ReturnType<typeof vi.fn>).mock.calls.length;
  await vi.advanceTimersByTimeAsync(1000);
  expect(
    (c.state as ReturnType<typeof vi.fn>).mock.calls.length - before,
  ).toBeGreaterThanOrEqual(3);
  ctl.dispose();
});
