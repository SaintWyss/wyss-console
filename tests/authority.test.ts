import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  parseAuthorityView,
  type AuthorityView,
} from '../src/lib/contracts/authority';
import { readAuthorityState } from '../src/lib/desktop/authority-client';
import {
  followAuthority,
  summarizeAgent,
  summarizeAuthority,
  type AuthorityObservation,
} from '../src/lib/shell/authority';
import fixtures from './fixtures/authority-view-v1.json';

const connected = parseAuthorityView(fixtures.connected);
const absent = parseAuthorityView(fixtures.waiting_absent);
const stopped = parseAuthorityView(fixtures.stopped);
const observed = (view: AuthorityView): AuthorityObservation => ({
  state: 'observed',
  view,
});
const settle = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
};

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

it('accepts exactly the views the desktop serializes', () => {
  expect(connected).toEqual(fixtures.connected);
  expect(absent).toEqual(fixtures.waiting_absent);
  expect(stopped).toEqual(fixtures.stopped);
  // A reason survives into the next attempt, until an authority answers.
  expect(
    parseAuthorityView({ ...fixtures.waiting_absent, connection: 'connecting' })
      .reason,
  ).toBe('absent');
  expect(
    parseAuthorityView({
      ...fixtures.waiting_absent,
      connection: 'synchronizing',
      reason: null,
    }).connection,
  ).toBe('synchronizing');
});

it('rejects anything that is not a v1 view', () => {
  const view = fixtures.connected;
  const rejected: [string, unknown][] = [
    ['view', null],
    ['view', []],
    ['view', 'connected'],
    ['view', { ...view, authority_instance_id: 'x' }],
    ['view', { ...view, authority: null }],
    ['view', { ...view, agent: null }],
    ['view', { ...fixtures.waiting_absent, agent: view.agent }],
    ['view', { ...fixtures.stopped, authority: view.authority }],
    ['schema_version', { ...view, schema_version: 2 }],
    ['revision', { ...view, revision: -1 }],
    ['revision', { ...view, revision: 1.5 }],
    ['revision', { ...view, revision: '3' }],
    ['connection', { ...view, connection: 'linked' }],
    ['reason', { ...view, reason: 'absent' }],
    ['reason', { ...fixtures.waiting_absent, reason: null }],
    ['reason', { ...fixtures.waiting_absent, reason: 'timeout' }],
    ['reason', { ...fixtures.stopped, reason: 'lost' }],
    [
      'authority',
      { ...view, authority: { ...view.authority, stream_id: 'x' } },
    ],
    [
      'service_version',
      { ...view, authority: { ...view.authority, service_version: 'a\nb' } },
    ],
    [
      'service_version',
      {
        ...view,
        authority: { ...view.authority, service_version: 'x'.repeat(65) },
      },
    ],
    [
      'last_sequence',
      { ...view, authority: { ...view.authority, last_sequence: 1 } },
    ],
    [
      'last_sequence',
      { ...view, authority: { ...view.authority, last_sequence: '01' } },
    ],
    [
      'prepared_sessions',
      { ...view, authority: { ...view.authority, prepared_sessions: -1 } },
    ],
    ['agent', { ...view, agent: { ...view.agent, workspace_root: '/home' } }],
    [
      'agent',
      { ...view, agent: { ...view.agent, diagnostic: 'spawn_failed' } },
    ],
    ['adapter', { ...view, agent: { ...view.agent, adapter: '<b>codex</b>' } }],
    ['desired', { ...view, agent: { ...view.agent, desired: 'paused' } }],
    ['actual', { ...view, agent: { ...view.agent, actual: 'running' } }],
    [
      'runtime_version',
      { ...view, agent: { ...view.agent, runtime_version: '\u001b[31m' } },
    ],
    ['model_count', { ...view, agent: { ...view.agent, model_count: 0.5 } }],
  ];
  for (const [field, value] of rejected)
    expect(() => parseAuthorityView(value), JSON.stringify(value)).toThrow(
      `authority_view: invalid_${field}`,
    );
});

it('reads through one narrow command and validates what comes back', async () => {
  const calls: unknown[] = [];
  mockIPC((command, payload) => {
    calls.push([command, payload]);
    return calls.length === 1 ? fixtures.connected : { result: 'ok' };
  });
  await expect(readAuthorityState(null)).resolves.toEqual(connected);
  await expect(readAuthorityState(3)).rejects.toThrow(
    'authority_view: invalid_view',
  );
  expect(calls).toEqual([
    ['authority_state', { request: { after: null } }],
    ['authority_state', { request: { after: 3 } }],
  ]);
});

it('maps every link state to one honest authority status', () => {
  const label = (o: AuthorityObservation) => {
    const { key, label, tone } = summarizeAuthority(o);
    return [key, label, tone];
  };
  expect(label({ state: 'pending' })).toEqual([
    'Authority',
    'Connecting',
    'pending',
  ]);
  expect(label({ state: 'unavailable' })).toEqual([
    'Authority',
    'Unavailable',
    'error',
  ]);
  expect(label(observed(connected))).toEqual(['Authority', 'Connected', 'ok']);
  expect(label(observed(stopped))).toEqual(['Authority', 'Stopped', 'idle']);
  const waiting = (reason: AuthorityView['reason']) =>
    label(observed({ ...absent, reason }));
  expect(waiting('absent')).toEqual(['Authority', 'Offline', 'idle']);
  expect(waiting('lost')).toEqual(['Authority', 'Reconnecting', 'warn']);
  expect(waiting('incompatible')).toEqual([
    'Authority',
    'Incompatible',
    'error',
  ]);
  expect(waiting('malformed')).toEqual(['Authority', 'Rejected', 'error']);
  expect(waiting('unsafe_path')).toEqual(['Authority', 'Unsafe', 'error']);
  // Between attempts the last reason stays: the status does not flicker.
  expect(label(observed({ ...absent, connection: 'connecting' }))).toEqual([
    'Authority',
    'Offline',
    'idle',
  ]);
  expect(
    label(observed({ ...absent, connection: 'connecting', reason: null })),
  ).toEqual(['Authority', 'Connecting', 'pending']);
  expect(
    label(observed({ ...absent, connection: 'synchronizing', reason: null })),
  ).toEqual(['Authority', 'Syncing', 'pending']);
});

it('summarizes sessions and the event head only from a connected view', () => {
  const summary = summarizeAuthority(observed(connected));
  expect(summary.note).toBe('1 session');
  expect(summary.detail).toBe(
    'Wyss authority 9.9.9-test · protocol 1 · sessions: 1 prepared, 0 closed · event head 1',
  );
  const busy = summarizeAuthority(
    observed({
      ...connected,
      authority: {
        ...connected.authority!,
        service_version: null,
        last_sequence: null,
        prepared_sessions: 3,
        closed_sessions: 2,
      },
    }),
  );
  expect(busy.note).toBe('3 sessions');
  expect(busy.detail).toBe(
    'Wyss authority · protocol 1 · sessions: 3 prepared, 2 closed · event head 0',
  );
  expect(summarizeAuthority(observed(absent)).note).toBeUndefined();
});

it('reports the managed agent only as the authority reports it', () => {
  const agent = (actual: NonNullable<AuthorityView['agent']>['actual']) => {
    const { key, label, tone } = summarizeAgent(
      observed({ ...connected, agent: { ...connected.agent!, actual } }),
    );
    return [key, label, tone];
  };
  expect(agent('ready')).toEqual(['Codex', 'Ready', 'ok']);
  expect(agent('starting')).toEqual(['Codex', 'Starting', 'pending']);
  expect(agent('initializing')).toEqual(['Codex', 'Initializing', 'pending']);
  expect(agent('stopping')).toEqual(['Codex', 'Stopping', 'pending']);
  expect(agent('stopped')).toEqual(['Codex', 'Stopped', 'idle']);
  expect(agent('failed')).toEqual(['Codex', 'Failed', 'error']);
  expect(
    summarizeAgent(
      observed({
        ...connected,
        agent: {
          adapter: 'codex',
          desired: 'running',
          actual: 'ready',
          runtime_version: '0.159.0',
          model_count: 7,
        },
      }),
    ).detail,
  ).toBe(
    'Codex 0.159.0 · desired running · 7 models · managed by the authority',
  );
  // No authority, no agent status: nothing stale and nothing guessed.
  for (const unknown of [
    { state: 'pending' },
    { state: 'unavailable' },
    observed(absent),
    observed(stopped),
  ] as AuthorityObservation[])
    expect(summarizeAgent(unknown)).toMatchObject({
      key: 'Codex',
      label: 'Unknown',
      tone: 'idle',
    });
});

it('follows revisions with one read in flight and its own cursor', async () => {
  const afters: (number | null)[] = [];
  const pending: ((view: AuthorityView) => void)[] = [];
  const seen: AuthorityObservation[] = [];
  const stop = followAuthority((o) => seen.push(o), {
    read: (after) => {
      afters.push(after);
      return new Promise((resolve) => pending.push(resolve));
    },
  });
  expect(afters).toEqual([null]);
  pending.shift()!(absent);
  await settle();
  expect(afters).toEqual([null, 1]);
  expect(pending).toHaveLength(1);
  pending.shift()!(connected);
  await settle();
  expect(afters).toEqual([null, 1, 3]);
  expect(seen).toEqual([observed(absent), observed(connected)]);
  // The bridge answered with nothing new: no observation and a bounded pause.
  pending.shift()!(connected);
  await settle();
  expect(seen).toHaveLength(2);
  expect(afters).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(249);
  expect(afters).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(1);
  expect(afters).toEqual([null, 1, 3, 3]);
  stop();
  pending.shift()!(stopped);
  await settle();
  await vi.advanceTimersByTimeAsync(5000);
  expect(seen).toHaveLength(2);
  expect(afters).toHaveLength(4);
});

it('reports a failed bridge, retries at a bounded pace and recovers', async () => {
  let fail = true;
  const afters: (number | null)[] = [];
  const seen: AuthorityObservation[] = [];
  const stop = followAuthority((o) => seen.push(o), {
    read: (after) => {
      afters.push(after);
      return fail
        ? Promise.reject(new Error('authority_view: invalid_view'))
        : Promise.resolve(stopped);
    },
  });
  await settle();
  expect(seen).toEqual([{ state: 'unavailable' }]);
  await vi.advanceTimersByTimeAsync(999);
  expect(afters).toEqual([null]);
  await vi.advanceTimersByTimeAsync(1);
  expect(afters).toEqual([null, null]);
  fail = false;
  await vi.advanceTimersByTimeAsync(1000);
  expect(seen.at(-1)).toEqual(observed(stopped));
  // A stopped service ends the loop: nothing is read again.
  const reads = afters.length;
  await vi.advanceTimersByTimeAsync(60_000);
  expect(afters).toHaveLength(reads);
  stop();
});
