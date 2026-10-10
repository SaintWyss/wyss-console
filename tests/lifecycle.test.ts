import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  LIFECYCLE_FAILURES,
  parseLifecycleView,
  type LifecycleView,
} from '../src/lib/contracts/lifecycle';
import {
  readLifecycle,
  retryAuthority,
} from '../src/lib/desktop/lifecycle-client';
import type { AuthorityObservation } from '../src/lib/shell/authority';
import {
  followLifecycle,
  lifecycleNotice,
  summarizeLink,
  type LifecycleObservation,
} from '../src/lib/shell/lifecycle';
import { parseAuthorityView } from '../src/lib/contracts/authority';
import views from './fixtures/authority-view-v1.json';
import { lifecycle } from './lifecycle-harness';

const observed = (over: Partial<LifecycleView> = {}): LifecycleObservation => ({
  state: 'observed',
  view: lifecycle(over),
});
const link = (name: keyof typeof views): AuthorityObservation => ({
  state: 'observed',
  view: parseAuthorityView(views[name]),
});

afterEach(() => {
  clearMocks();
  vi.useRealTimers();
});

it('accepts the view the desktop sends and nothing beside it', () => {
  const ready = lifecycle({
    phase: 'ready',
    origin: 'managed',
    update_pending: true,
    diagnostics: {
      build: 'packaged',
      installation: 'outdated',
      service: 'running',
    },
  });
  expect(parseLifecycleView(ready)).toEqual(ready);
  for (const failure of LIFECYCLE_FAILURES) {
    const failed = lifecycle({ phase: 'failed', failure });
    expect(parseLifecycleView(failed)).toEqual(failed);
  }
  // Nothing of the system underneath crosses, under any name.
  for (const extra of [
    'pid',
    'unit',
    'service_name',
    'path',
    'binary',
    'command',
    'socket',
    'error',
    'stderr',
    'environment',
    'digest',
  ]) {
    expect(() => parseLifecycleView({ ...ready, [extra]: 'x' })).toThrow(
      /invalid_view/,
    );
    expect(() =>
      parseLifecycleView({
        ...ready,
        diagnostics: { ...ready.diagnostics, [extra]: 'x' },
      }),
    ).toThrow(/invalid_diagnostics/);
  }
  const bad = (over: Record<string, unknown>, why: RegExp) =>
    expect(() => parseLifecycleView({ ...lifecycle(), ...over })).toThrow(why);
  bad({ schema_version: 2 }, /schema_version/);
  bad({ revision: -1 }, /revision/);
  bad({ revision: '1' }, /revision/);
  bad({ phase: 'running' }, /invalid_phase/);
  bad({ phase: 'connected' }, /invalid_phase/);
  bad({ phase: 'failed', failure: 'because' }, /invalid_failure/);
  bad(
    {
      diagnostics: { build: 'x', installation: 'unknown', service: 'unknown' },
    },
    /invalid_build/,
  );
  bad(
    {
      diagnostics: {
        build: 'packaged',
        installation: '/home/x/.local/share',
        service: 'unknown',
      },
    },
    /invalid_installation/,
  );
  bad(
    {
      diagnostics: {
        build: 'packaged',
        installation: 'unknown',
        service: 'wyss-control.service',
      },
    },
    /invalid_service/,
  );
  expect(() => parseLifecycleView(null)).toThrow(/invalid_view/);
  expect(() => parseLifecycleView([])).toThrow(/invalid_view/);
});

it('refuses a view that contradicts itself', () => {
  const bad = (over: Partial<LifecycleView>, why: RegExp) =>
    expect(() => parseLifecycleView(lifecycle(over))).toThrow(why);
  // A failure is said exactly while it failed, and Retry is offered exactly then.
  bad({ phase: 'failed', failure: null }, /invalid_failure/);
  bad({ phase: 'ready', failure: 'stopped', retry: false }, /invalid_failure/);
  bad({ phase: 'failed', failure: 'stopped', retry: false }, /invalid_retry/);
  bad({ phase: 'starting', retry: true }, /invalid_retry/);
  bad({ phase: 'ready', retry: true }, /invalid_retry/);
  // Who started an authority is known only of one that answers.
  for (const phase of [
    'checking',
    'installing',
    'starting',
    'connecting',
  ] as const)
    bad({ phase, origin: 'managed' }, /invalid_origin/);
  bad(
    { phase: 'failed', failure: 'stopped', origin: 'external' },
    /invalid_origin/,
  );
  // An update waits only for this installation's own authority, while it is ready.
  bad(
    { phase: 'ready', origin: 'external', update_pending: true },
    /update_pending/,
  );
  bad({ phase: 'ready', origin: null, update_pending: true }, /update_pending/);
  bad(
    { phase: 'incompatible', origin: 'managed', update_pending: true },
    /update_pending/,
  );
});

it('reaches the lifecycle through one read and one intent that name nothing', async () => {
  const calls: { command: string; payload: unknown }[] = [];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    return lifecycle({ phase: 'failed', failure: 'stopped', revision: 4 });
  });
  expect((await readLifecycle(null)).revision).toBe(4);
  expect((await readLifecycle(4)).failure).toBe('stopped');
  expect((await retryAuthority()).phase).toBe('failed');
  expect(calls).toEqual([
    { command: 'authority_lifecycle', payload: { request: { after: null } } },
    { command: 'authority_lifecycle', payload: { request: { after: 4 } } },
    // Retry carries nothing: no service, no command, no path, no argument.
    { command: 'authority_retry', payload: { request: {} } },
  ]);
  // An answer that is not the view is a failed bridge, never data.
  mockIPC(() => ({ phase: 'ready', pid: 4242 }));
  await expect(readLifecycle(null)).rejects.toThrow(/invalid_view/);
  await expect(retryAuthority()).rejects.toThrow(/invalid_view/);
});

it('follows the lifecycle with one read at a time and starts nothing by itself', async () => {
  vi.useFakeTimers();
  const pending: {
    after: number | null;
    resolve: (view: LifecycleView) => void;
    reject: (error: unknown) => void;
  }[] = [];
  const read = (after: number | null) =>
    new Promise<LifecycleView>((resolve, reject) =>
      pending.push({ after, resolve, reject }),
    );
  const seen: LifecycleObservation[] = [];
  const stop = followLifecycle((o) => seen.push(o), { read });
  // One read, waiting in the desktop. Time passing does not make another.
  expect(pending.map((p) => p.after)).toEqual([null]);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(pending).toHaveLength(1);
  expect(seen).toEqual([]);

  pending[0].resolve(lifecycle({ phase: 'starting', revision: 2 }));
  await vi.advanceTimersByTimeAsync(0);
  expect(seen).toHaveLength(1);
  // The next read waits for what comes after the revision it has.
  expect(pending.map((p) => p.after)).toEqual([null, 2]);
  // Answered unchanged (the desktop's own wait ended): nothing is told again, and the
  // next read is not made in a loop.
  pending[1].resolve(lifecycle({ phase: 'starting', revision: 2 }));
  await vi.advanceTimersByTimeAsync(0);
  expect(seen).toHaveLength(1);
  expect(pending).toHaveLength(2);
  await vi.advanceTimersByTimeAsync(250);
  expect(pending).toHaveLength(3);

  // The bridge fails: that is said, and it is asked again from the start, later.
  pending[2].reject(new Error('bridge'));
  await vi.advanceTimersByTimeAsync(0);
  expect(seen.at(-1)).toEqual({ state: 'unavailable' });
  expect(pending).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(1000);
  expect(pending.map((p) => p.after)).toEqual([null, 2, 2, null]);
  pending[3].resolve(lifecycle({ phase: 'ready', revision: 5 }));
  await vi.advanceTimersByTimeAsync(0);
  expect(seen.at(-1)).toEqual(observed({ phase: 'ready', revision: 5 }));
  // Ready: one read waits, and nothing else happens however long it is.
  expect(pending).toHaveLength(5);
  await vi.advanceTimersByTimeAsync(600_000);
  expect(pending).toHaveLength(5);

  // Disposed: an answer that arrives late is dropped and nothing is asked again.
  stop();
  pending[4].resolve(
    lifecycle({ phase: 'failed', failure: 'stopped', revision: 6 }),
  );
  await vi.advanceTimersByTimeAsync(5000);
  expect(pending).toHaveLength(5);
  expect(seen).toHaveLength(3);
});

it('says connected only when the link is, whatever the lifecycle says', () => {
  const connected = link('connected');
  const absent = link('waiting_absent');
  const pending: AuthorityObservation = { state: 'pending' };
  // The lifecycle says ready and the link does not: nothing is shown as connected.
  for (const authority of [absent, pending])
    expect(
      summarizeLink(authority, observed({ phase: 'ready' })).label,
    ).not.toBe('Connected');
  expect(summarizeLink(absent, observed({ phase: 'ready' })).label).toBe(
    'Offline',
  );
  // The link is connected: that is what is shown, in every phase the lifecycle is in.
  for (const phase of ['checking', 'starting', 'connecting', 'ready'] as const)
    expect(summarizeLink(connected, observed({ phase }))).toMatchObject({
      label: 'Connected',
      tone: 'ok',
    });
  expect(
    summarizeLink(connected, observed({ phase: 'failed', failure: 'stopped' }))
      .label,
  ).toBe('Connected');
  // While nothing answers, what is being done about it.
  expect(
    summarizeLink(absent, observed({ phase: 'installing' })),
  ).toMatchObject({
    label: 'Installing',
    tone: 'pending',
  });
  for (const phase of ['starting', 'connecting'] as const)
    expect(summarizeLink(absent, observed({ phase }))).toMatchObject({
      label: 'Starting',
      tone: 'pending',
    });
  // And why nothing does. A stop and a development build are not errors.
  expect(
    summarizeLink(
      absent,
      observed({ phase: 'failed', failure: 'startup_failed' }),
    ),
  ).toMatchObject({ label: 'Failed to start', tone: 'error' });
  for (const failure of ['stopped', 'not_packaged'] as const)
    expect(
      summarizeLink(absent, observed({ phase: 'failed', failure })),
    ).toMatchObject({ label: 'Offline', tone: 'idle' });
  expect(
    summarizeLink(absent, observed({ phase: 'incompatible' })),
  ).toMatchObject({
    label: 'Incompatible',
    tone: 'error',
  });
  // Before the lifecycle is known, the link's own word stands.
  expect(summarizeLink(absent, { state: 'pending' }).label).toBe('Offline');
  expect(summarizeLink(absent, { state: 'unavailable' }).label).toBe('Offline');
  // Who started it, and that a newer one waits, are said about a connected authority.
  const managed = summarizeLink(
    connected,
    observed({ phase: 'ready', origin: 'managed', update_pending: true }),
  );
  expect(managed.note).toBe('update pending');
  expect(managed.detail).toContain('started by Wyss Console');
  expect(managed.detail).toContain(
    'a newer authority is installed the next time it starts',
  );
  const external = summarizeLink(
    connected,
    observed({ phase: 'ready', origin: 'external' }),
  );
  expect(external.detail).toContain('started outside Wyss Console');
  expect(external.note).not.toBe('update pending');
});

it('says why the authority is not available, in words and never in commands', () => {
  const absent = link('waiting_absent');
  // Nothing to say while it is being made available, or before anything is known.
  for (const phase of [
    'checking',
    'installing',
    'starting',
    'connecting',
    'ready',
  ] as const)
    expect(lifecycleNotice(observed({ phase }), absent)).toBeNull();
  expect(lifecycleNotice({ state: 'pending' }, absent)).toBeNull();
  expect(lifecycleNotice({ state: 'unavailable' }, absent)).toBeNull();
  // And nothing while the link is connected, whatever an older view says.
  expect(
    lifecycleNotice(
      observed({ phase: 'failed', failure: 'startup_failed' }),
      link('connected'),
    ),
  ).toBeNull();

  const texts: string[] = [];
  for (const failure of LIFECYCLE_FAILURES) {
    const notice = lifecycleNotice(
      observed({ phase: 'failed', failure }),
      absent,
    )!;
    expect(notice.retry).toBe(true);
    expect(notice.text.length).toBeGreaterThan(20);
    texts.push(
      notice.title,
      notice.text,
      ...notice.details.map((d) => d.value),
    );
  }
  expect(
    lifecycleNotice(observed({ phase: 'failed', failure: 'stopped' }), absent),
  ).toMatchObject({ tone: 'idle', title: 'The Wyss authority is not running' });
  expect(
    lifecycleNotice(
      observed({ phase: 'failed', failure: 'exited_repeatedly' }),
      absent,
    ),
  ).toMatchObject({
    tone: 'error',
    title: 'The Wyss authority could not start',
    text: 'The authority ended repeatedly and is no longer being restarted.',
  });
  // An incompatible authority is said, and there is nothing to retry against it.
  const foreign = lifecycleNotice(
    observed({ phase: 'incompatible', origin: 'external' }),
    absent,
  )!;
  expect(foreign).toMatchObject({
    tone: 'error',
    title: 'An incompatible Wyss authority is running',
    retry: false,
  });
  expect(foreign.text).toContain('left exactly as it is');
  const own = lifecycleNotice(
    observed({ phase: 'incompatible', origin: 'managed' }),
    absent,
  )!;
  expect(own.text).toContain('replaced the next time it starts');
  expect(own.text).toContain('not restarted under work');
  texts.push(foreign.text, own.text);
  // The details are three plain facts.
  expect(
    lifecycleNotice(
      observed({
        phase: 'failed',
        failure: 'manager_unavailable',
        diagnostics: {
          build: 'packaged',
          installation: 'absent',
          service: 'unavailable',
        },
      }),
      absent,
    )!.details,
  ).toEqual([
    { term: 'This build', value: 'Packaged: carries its own authority' },
    { term: 'Installation', value: 'Not installed' },
    { term: 'Service', value: 'No service manager answered' },
  ]);
  // No command line, unit, path, process or socket in anything a person is shown.
  for (const text of texts)
    expect(text).not.toMatch(
      /systemctl|systemd|\.service|d-?bus|\bpid\b|\/|~|\.sock|XDG|journal|sudo|--/i,
    );
});

beforeEach(() => {
  clearMocks();
});
