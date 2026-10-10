import { afterEach, expect, it } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  EVENT_KIND_CATEGORY,
  EVENTS_PAGE_LIMIT,
  parseEventRow,
  parseEventsPage,
  type EventKind,
} from '../src/lib/contracts/events';
import { readEvents, readOlderEvents } from '../src/lib/desktop/events-client';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import {
  A1,
  OTHER_STREAM,
  rawPage,
  row,
  S1,
  STREAM,
  T1,
} from './events-harness';

afterEach(() => clearMocks());

const tampered = (value: unknown, path: (string | number)[], next: unknown) => {
  const copy = structuredClone(value) as Record<string | number, unknown>;
  let at = copy;
  for (const key of path.slice(0, -1))
    at = at[key] as Record<string | number, unknown>;
  at[path.at(-1)!] = next;
  return copy;
};

it('accepts the projection the desktop sends, positions as the text they are', () => {
  const raw = rawPage(120, 50);
  const parsed = parseEventsPage(raw);
  expect(parsed).toEqual(raw);
  expect(parsed.events.map((e) => e.sequence).slice(0, 3)).toEqual([
    '120',
    '119',
    '118',
  ]);
  expect(parsed.older).toBe('71');
  // A stream is named in lower case, however it was spelled.
  expect(parseEventsPage({ ...raw, stream: STREAM.toUpperCase() }).stream).toBe(
    STREAM,
  );
  // The beginning of a stream, and no events at all.
  expect(parseEventsPage(rawPage(30, 30)).older).toBeNull();
  expect(parseEventsPage(rawPage(0, 0))).toMatchObject({
    head: null,
    events: [],
    older: null,
  });
  // A window that begins at a hole says so, and has nothing to hand back.
  expect(parseEventsPage(rawPage(120, 40, { gap: true }))).toMatchObject({
    gap: true,
    older: null,
  });
  // Positions past what a number holds exactly are still themselves: they were never
  // numbers. These two differ by one and no float can tell them apart.
  const far = {
    ...rawPage(2, 2),
    head: '18446744073709551615',
    events: [row('18446744073709551615'), row('18446744073709551614')],
    older: '18446744073709551614',
  };
  const kept = parseEventsPage(far);
  expect(kept.events.map((e) => e.sequence)).toEqual([
    '18446744073709551615',
    '18446744073709551614',
  ]);
  expect(kept.older).toBe('18446744073709551614');
  expect(typeof kept.events[0].sequence).toBe('string');
  // And two that a float would call consecutive, or equal, are not accepted as a run.
  expect(() =>
    parseEventsPage({
      ...far,
      events: [row('18446744073709551615'), row('18446744073709551613')],
      older: '18446744073709551613',
    }),
  ).toThrow(/invalid_order/);
  // Nor is an event one past the head taken for the head itself.
  expect(() =>
    parseEventsPage({ ...far, head: '18446744073709551614' }),
  ).toThrow(/invalid_head/);
});

it('refuses anything a row of history has no use for', () => {
  const raw = rawPage(12, 3);
  // What the authority stores about an event never crosses, under any name.
  for (const extra of [
    'payload',
    'event_type',
    'event_id',
    'lease_id',
    'action_fingerprint',
    'runtime_instance_id',
    'codex_turn_id',
    'runtime_session_id',
    'operation_id',
    'command',
    'output',
    'reasoning',
    'prompt',
    'summary',
    'source',
  ]) {
    expect(() =>
      parseEventsPage(tampered(raw, ['events', 0, extra], 'x')),
    ).toThrow(/invalid_row/);
    expect(() =>
      parseEventsPage(tampered(raw, ['events', 0, 'facts', extra], 'x')),
    ).toThrow(/invalid_facts/);
    expect(() => parseEventsPage({ ...raw, [extra]: 'x' })).toThrow(
      /invalid_page/,
    );
  }
  // And nothing may be missing.
  for (const field of ['sequence', 'at', 'facts', 'security']) {
    const less = structuredClone(raw) as unknown as {
      events: Record<string, unknown>[];
    };
    delete less.events[0][field];
    expect(() => parseEventsPage(less)).toThrow(/invalid_row/);
  }
});

it('refuses a page that is not an unbroken run of one stream, newest first', () => {
  const raw = rawPage(12, 4);
  const bad = (path: (string | number)[], next: unknown, why: RegExp) =>
    expect(() => parseEventsPage(tampered(raw, path, next))).toThrow(why);
  bad(['schema_version'], 2, /schema_version/);
  bad(['stream'], 'the-stream', /invalid_stream/);
  bad(['stream'], null, /invalid_stream/);
  bad(['gap'], 'no', /invalid_gap/);
  // A position is canonical decimal text.
  for (const position of [12, '012', '0', '-1', '1e3', ' 12', '', null])
    bad(['events', 0, 'sequence'], position, /invalid_(sequence|order|head)/);
  bad(['head'], 12, /invalid_head/);
  bad(['older'], 9, /invalid_older/);
  // A time is the authority's UTC timestamp, not anything a date can be read from.
  for (const at of [
    '2026-10-09 10:00:00Z',
    'October 9, 2026 10:00 UTC',
    '2026-10-09',
    '2026-10-09T10:00:00+02:00',
    '2026-13-40T10:00:00Z',
    1791590922,
    null,
  ])
    bad(['events', 0, 'at'], at, /invalid_at/);
  // A hole, a repeat, an ascending pair.
  bad(['events', 2, 'sequence'], '9', /invalid_order/);
  bad(['events', 1, 'sequence'], '12', /invalid_order/);
  expect(() =>
    parseEventsPage({ ...raw, events: [...raw.events].reverse() }),
  ).toThrow(/invalid_order/);
  // Newer than the head that was read before it.
  bad(['head'], '11', /invalid_head/);
  bad(['head'], null, /invalid_head/);
  // The position to hand back is the oldest event, and only where there is one before.
  bad(['older'], '10', /invalid_older/);
  bad(['older'], null, /invalid_older/);
  expect(() => parseEventsPage({ ...rawPage(3, 3), older: '1' })).toThrow(
    /invalid_older/,
  );
  expect(() =>
    parseEventsPage({ ...rawPage(12, 4, { gap: true }), older: '9' }),
  ).toThrow(/invalid_older/);
  // A window none of which is there is a gap with nothing in it, and hands nothing back.
  expect(
    parseEventsPage({ ...rawPage(0, 0), head: '120', gap: true }),
  ).toMatchObject({ events: [], gap: true, older: null });
  expect(() =>
    parseEventsPage({ ...rawPage(0, 0), head: '120', gap: true, older: '70' }),
  ).toThrow(/invalid_older/);
  // Never more than a page.
  expect(parseEventsPage(rawPage(200, EVENTS_PAGE_LIMIT)).events).toHaveLength(
    EVENTS_PAGE_LIMIT,
  );
  expect(() => parseEventsPage(rawPage(200, EVENTS_PAGE_LIMIT + 1))).toThrow(
    /invalid_events/,
  );
});

it('refuses a row whose parts contradict each other', () => {
  const bad = (value: unknown, why: RegExp) =>
    expect(() => parseEventRow(value)).toThrow(why);
  // Every kind has one category.
  for (const kind of Object.keys(EVENT_KIND_CATEGORY) as EventKind[]) {
    expect(parseEventRow(row(5, kind)).category).toBe(
      EVENT_KIND_CATEGORY[kind],
    );
    const wrong = EVENT_KIND_CATEGORY[kind] === 'turn' ? 'session' : 'turn';
    bad(
      { ...row(5, kind), category: wrong, turn: null, approval: null },
      /invalid_category/,
    );
  }
  bad({ ...row(5), kind: 'turn_exploded' }, /invalid_kind/);
  bad({ ...row(5), kind: 'authority.managed_turn.running' }, /invalid_kind/);
  bad({ ...row(5), disposition: 'critical' }, /invalid_disposition/);
  bad({ ...row(5), security: 'yes' }, /invalid_security/);
  bad({ ...row(5), at: 'yesterday' }, /invalid_at/);
  // Identities only where the event is about them.
  bad({ ...row(5, 'session_prepared'), turn: T1 }, /invalid_turn/);
  bad({ ...row(5, 'runtime_ready'), turn: T1 }, /invalid_turn/);
  bad({ ...row(5, 'turn_completed'), approval: A1 }, /invalid_approval/);
  bad({ ...row(5, 'lease_granted'), approval: A1 }, /invalid_approval/);
  bad({ ...row(5), session: 'not-a-session' }, /./);
  // A kind this build does not know carries nothing.
  expect(parseEventRow(row(5, 'other', { session: S1 })).session).toBe(S1);
  bad(
    row(5, 'other', { facts: { turn_reason: 'user_cancel' } }),
    /invalid_other/,
  );
  bad(row(5, 'other', { facts: { runtime_version: '1.0' } }), /invalid_other/);
  bad(row(5, 'other', { disposition: 'failure' }), /invalid_other/);
  bad(row(5, 'other', { security: true }), /invalid_other/);
  bad(row(5, 'other', { turn: T1 }), /invalid_turn/);
  // Facts come from closed vocabularies.
  const fact = (name: string, value: unknown) => ({
    ...row(5),
    facts: { ...row(5).facts, [name]: value },
  });
  bad(fact('turn_reason', 'because'), /invalid_turn_reason/);
  bad(fact('approval_kind', 'shell'), /invalid_approval_kind/);
  bad(fact('approval_reason', 7), /invalid_approval_reason/);
  bad(fact('approval_effect', 'done'), /invalid_approval_effect/);
  bad(fact('desired', 'paused'), /invalid_desired/);
  bad(fact('exit', 'segfault'), /invalid_exit/);
  bad(fact('expected', 'true'), /invalid_expected/);
  bad(fact('revocation', 'because'), /invalid_revocation/);
  bad(fact('expires_at', 'tomorrow'), /invalid_expires_at/);
  for (const version of ['', '0.1 beta', '<b>1</b>', 'x'.repeat(33), 159])
    bad(fact('runtime_version', version), /invalid_runtime_version/);
  bad(fact('scopes', 'turn_start'), /invalid_scopes/);
  bad(fact('scopes', ['turn_start', 'turn_start']), /invalid_scopes/);
  bad(fact('scopes', ['turn_start', 'root']), /invalid_scopes/);
  bad(fact('scopes', Array(6).fill('turn_start')), /invalid_scopes/);
});

interface Call {
  command: string;
  payload: unknown;
}
function bridge(answer: (call: Call) => unknown) {
  const calls: Call[] = [];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    return answer({ command, payload });
  });
  return calls;
}
async function failure(call: Promise<unknown>) {
  try {
    await call;
  } catch (error) {
    expect(error).toBeInstanceOf(SessionCallError);
    return (error as SessionCallError).failure;
  }
  throw new Error('the call was answered');
}

it('reads history through two narrow requests and nothing else', async () => {
  const calls = bridge(({ command }) =>
    command === 'managed_events' ? rawPage(120, 50) : rawPage(70, 50),
  );
  expect((await readEvents()).events).toHaveLength(50);
  const older = await readOlderEvents(STREAM, '71');
  expect(older.events[0].sequence).toBe('70');
  expect(calls).toEqual([
    { command: 'managed_events', payload: { request: {} } },
    {
      command: 'managed_events_older',
      // The position goes back as the text it came as.
      payload: { request: { stream_id: STREAM, before: '71' } },
    },
  ]);
});

it('takes an answer about another stream or another window for a failed bridge', async () => {
  const internal = { code: 'internal', retry: false };
  // Another stream.
  bridge(() => ({ ...rawPage(70, 50), stream: OTHER_STREAM }));
  expect(await failure(readOlderEvents(STREAM, '71'))).toEqual(internal);
  // Not the window that ends right before the position asked for.
  bridge(() => rawPage(69, 49));
  expect(await failure(readOlderEvents(STREAM, '71'))).toEqual(internal);
  bridge(() => rawPage(71, 50));
  expect(await failure(readOlderEvents(STREAM, '71'))).toEqual(internal);
  // Far positions are compared as what they are: this is the window right before.
  bridge(() => ({
    ...rawPage(2, 2),
    head: '18446744073709551615',
    events: [row('18446744073709551614'), row('18446744073709551613')],
    older: '18446744073709551613',
  }));
  expect(
    (await readOlderEvents(STREAM, '18446744073709551615')).events[0].sequence,
  ).toBe('18446744073709551614');
  // A window none of which is there to read is a fair answer about older history.
  bridge(() => ({ ...rawPage(0, 0), head: '120', gap: true }));
  expect(await readOlderEvents(STREAM, '71')).toMatchObject({
    events: [],
    gap: true,
  });
  // It is not one about the newest: a stream that has a head shows its head.
  expect(await failure(readEvents())).toEqual(internal);
  bridge(() => ({ ...rawPage(0, 0), head: '120' }));
  expect(await failure(readEvents())).toEqual(internal);
  // Nothing before the first event is a fair answer.
  bridge(() => rawPage(0, 0, { head: '9' }));
  expect((await readOlderEvents(STREAM, '1')).events).toEqual([]);
  // Not the projection at all: an envelope as the authority stores it.
  bridge(() => ({
    events: [{ event_type: 'authority.managed_turn.running', payload: {} }],
    next: null,
  }));
  expect(await failure(readEvents())).toEqual(internal);
  bridge(() => null);
  expect(await failure(readEvents())).toEqual(internal);
});

it('reports a refusal by the name the desktop gave it', async () => {
  bridge(() => Promise.reject({ code: 'changed', retry: false }));
  expect(await failure(readOlderEvents(STREAM, '71'))).toEqual({
    code: 'changed',
    retry: false,
  });
  bridge(() => Promise.reject({ code: 'unavailable', retry: true }));
  expect(await failure(readEvents())).toEqual({
    code: 'unavailable',
    retry: true,
  });
  bridge(() => Promise.reject('boom'));
  expect((await failure(readEvents())).code).toBe('internal');
});
