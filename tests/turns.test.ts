import { afterEach, expect, it } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  parseManagedSessionId,
  parseManagedTurnId,
} from '../src/lib/contracts/identities';
import {
  type HistoryCursor,
  parseHistoryCursor,
  parseTurnHistory,
  parseTurnList,
  parseTurnReceipt,
  parseTurnView,
  TURN_EVENT_KINDS,
  TURN_REASONS,
  TURN_STATES,
  turnEnded,
} from '../src/lib/contracts/turns';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import {
  cancelTurn,
  readOlderTurns,
  readTurn,
  readTurns,
  startTurn,
} from '../src/lib/desktop/turns-client';
import fixture from './fixtures/turn-view-v2.json';

const SESSION = parseManagedSessionId(fixture.list.session.id);
const TURN = parseManagedTurnId(fixture.view.turn.id);
const OTHER = '99999999-9999-4999-8999-999999999999';
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
/** `base` with one path replaced, or removed when `value` is undefined. */
function tampered(base: unknown, path: (string | number)[], value: unknown) {
  const copy = clone(base) as Record<string | number, unknown>;
  let node = copy;
  for (const key of path.slice(0, -1))
    node = node[key] as Record<string | number, unknown>;
  const last = path.at(-1)!;
  if (value === undefined) delete node[last];
  else node[last] = value;
  return copy;
}

afterEach(() => clearMocks());

it('accepts the projection the desktop sends and nothing beside it', () => {
  expect(parseTurnView(fixture.view)).toEqual(fixture.view);
  expect(parseTurnView(fixture.waiting)).toEqual(fixture.waiting);
  expect(parseTurnList(fixture.list)).toEqual(fixture.list);
  expect(parseTurnList(fixture.empty)).toEqual(fixture.empty);
  // The lifecycle is the authority's own, by its own names.
  expect(TURN_STATES).toEqual([
    'starting',
    'running',
    'completed',
    'interrupted',
    'failed',
    'indeterminate',
  ]);
  expect(TURN_STATES.filter(turnEnded)).toEqual([
    'completed',
    'interrupted',
    'failed',
    'indeterminate',
  ]);
  expect(TURN_REASONS).toHaveLength(15);
  expect(TURN_EVENT_KINDS).toHaveLength(19);
});

it('refuses a reading that carries anything the window is not to be given', () => {
  // A provider frame, a provider identity, a lease, a runtime, reasoning: none has a
  // place in the projection, so a reading that brings one is not shown at all.
  for (const extra of [
    'codex_turn_id',
    'codex_thread_id',
    'admitted_lease_id',
    'lease_id',
    'runtime_instance_id',
    'reasoning',
    'raw',
    'frame',
    'managed_approval_id',
  ]) {
    expect(() => parseTurnView({ ...fixture.view, [extra]: 'x' })).toThrow();
    expect(() =>
      parseTurnView(tampered(fixture.view, ['turn', extra], 'x')),
    ).toThrow();
    expect(() =>
      parseTurnView(tampered(fixture.view, ['events', 0, extra], 'x')),
    ).toThrow();
    expect(() =>
      parseTurnView(tampered(fixture.waiting, ['approval', extra], 'x')),
    ).toThrow();
    expect(() => parseTurnList({ ...fixture.list, [extra]: 'x' })).toThrow();
    expect(() =>
      parseTurnList(tampered(fixture.list, ['turns', 0, extra], 'x')),
    ).toThrow();
    expect(() =>
      parseTurnList(tampered(fixture.list, ['session', extra], 'x')),
    ).toThrow();
  }
  // Only the answer carries text. Reasoning, a command or its output cannot ride on
  // another kind of event.
  for (const kind of TURN_EVENT_KINDS.filter(
    (k) => k !== 'text' && k !== 'approval_resolved',
  ))
    expect(() =>
      parseTurnView(
        tampered(fixture.view, ['events', 0], {
          sequence: '1',
          kind,
          text: 'PRIVATE',
        }),
      ),
    ).toThrow(/invalid_text/);
  // An approval's end is one of its states, never free text.
  const resolved = (text: unknown) =>
    tampered(fixture.view, ['events', 0], {
      sequence: '1',
      kind: 'approval_resolved',
      text,
    });
  expect(() => parseTurnView(resolved('accepted'))).not.toThrow();
  expect(() => parseTurnView(resolved(null))).not.toThrow();
  expect(() => parseTurnView(resolved('rm -rf /'))).toThrow(/invalid_text/);
  expect(() => parseTurnView(resolved('pending'))).toThrow(/invalid_text/);
  // A piece of the answer is text of bounded length and nothing else.
  for (const text of [null, 7, ['x'], { text: 'x' }, 'x'.repeat(4097)])
    expect(() =>
      parseTurnView(tampered(fixture.view, ['events', 2, 'text'], text)),
    ).toThrow(/invalid_text/);
  // An unknown kind of event, state or reason is not guessed at.
  expect(() =>
    parseTurnView(tampered(fixture.view, ['events', 0, 'kind'], 'reasoning')),
  ).toThrow(/invalid_kind/);
  expect(() =>
    parseTurnView(tampered(fixture.view, ['turn', 'state'], 'waiting')),
  ).toThrow(/invalid_state/);
  expect(() =>
    parseTurnList(tampered(fixture.list, ['turns', 0, 'reason'], 'because')),
  ).toThrow(/invalid_reason/);
});

it('refuses a reading whose parts contradict each other', () => {
  const view = (path: (string | number)[], value: unknown) => () =>
    parseTurnView(tampered(fixture.view, path, value));
  expect(view(['schema_version'], 1)).toThrow(/schema_version/);
  expect(view(['schema_version'], 3)).toThrow(/schema_version/);
  // Cursors are canonical decimal text, never numbers.
  expect(view(['next'], 5)).toThrow(/invalid_next/);
  expect(view(['next'], '05')).toThrow(/invalid_next/);
  expect(view(['events', 0, 'sequence'], 1)).toThrow(/invalid_sequence/);
  expect(view(['events', 0, 'sequence'], '0')).toThrow(/invalid_sequence/);
  // In stream order, each position once, and the cursor is where the reading ends.
  expect(view(['events', 1, 'sequence'], '1')).toThrow(/invalid_order/);
  expect(view(['events', 3, 'sequence'], '2')).toThrow(/invalid_order/);
  expect(view(['next'], '4')).toThrow(/invalid_next/);
  expect(view(['next'], null)).toThrow(/invalid_next/);
  // A turn that has not ended says neither when nor why it ended, and the reverse.
  expect(view(['turn', 'ended_at'], '2026-10-09T10:11:00Z')).toThrow(/end/);
  expect(view(['turn', 'reason'], 'completed')).toThrow(/end/);
  expect(view(['turn', 'cancellable'], false)).toThrow(/hints/);
  expect(view(['turn', 'revision'], 0)).toThrow(/revision/);
  expect(view(['turn', 'revision'], '2')).toThrow(/revision/);
  expect(view(['turn', 'started_at'], 'yesterday')).toThrow(/started_at/);
  expect(view(['turn', 'id'], 'turn-1')).toThrow(/invalid_id/);
  expect(view(['more'], 'no')).toThrow(/invalid_more/);
  expect(view(['gap'], 0)).toThrow(/invalid_gap/);
  expect(view(['gap'], undefined)).toThrow(/invalid_view/);
  expect(() =>
    parseTurnView({
      ...fixture.view,
      events: Array.from({ length: 51 }, (_, i) => ({
        sequence: String(i + 1),
        kind: 'text',
        text: 'x',
      })),
      next: '51',
    }),
  ).toThrow(/invalid_events/);
  // A turn that ended waits on nothing, and cannot be cancelled.
  const ended = {
    ...fixture.waiting,
    turn: {
      ...fixture.waiting.turn,
      state: 'completed',
      ended_at: '2026-10-09T10:11:00Z',
      reason: 'completed',
      cancellable: false,
    },
  };
  expect(() => parseTurnView(ended)).toThrow(/invalid_approval/);
  expect(() => parseTurnView({ ...ended, approval: null })).not.toThrow();
  expect(() =>
    parseTurnView(tampered(ended, ['turn', 'cancellable'], true)),
  ).toThrow(/hints/);
  expect(() =>
    parseTurnView(tampered(fixture.waiting, ['approval', 'kind'], 'network')),
  ).toThrow(/invalid_approval/);

  const list = (path: (string | number)[], value: unknown) => () =>
    parseTurnList(tampered(fixture.list, path, value));
  // Every turn listed is this session's, once.
  expect(list(['turns', 1, 'session'], OTHER)).toThrow(/invalid_turns/);
  expect(list(['turns', 1, 'id'], fixture.list.turns[0].id)).toThrow(
    /invalid_turns/,
  );
  expect(list(['head'], 14)).toThrow(/invalid_head/);
  expect(list(['partial'], null)).toThrow(/invalid_partial/);
  // No hint offers a turn that what is shown already rules out.
  expect(list(['session', 'binding'], 'bound_detached')).toThrow(/mode/);
  expect(() =>
    parseTurnList({
      ...fixture.list,
      session: {
        ...fixture.list.session,
        binding: 'bound_detached',
        mode: 'unknown',
      },
    }),
  ).toThrow(/hints/);
  // A mode is one of the product's two, or none while the session is not attached.
  expect(list(['session', 'mode'], 'codex_local_tools_v1')).toThrow(/mode/);
  expect(list(['session', 'mode'], undefined)).toThrow(/session/);
  expect(() =>
    parseTurnList(tampered(fixture.list, ['session', 'mode'], 'local_tools')),
  ).not.toThrow();
  expect(
    list(['turns', 0], { ...fixture.view.turn, session: SESSION }),
  ).toThrow(/hints/);
  expect(() =>
    parseTurnList({
      ...fixture.list,
      runnable: false,
      session: { ...fixture.list.session, state: 'closed', revision: 2 },
    }),
  ).not.toThrow();
  expect(list(['session', 'revision'], 2)).toThrow(/revision/);
  expect(list(['session', 'workspace'], null)).toThrow(/workspace/);
  expect(list(['session', 'workspace'], 'relative/path')).toThrow(/workspace/);
  expect(() =>
    parseTurnList({
      ...fixture.list,
      turns: Array.from({ length: 21 }, () => fixture.list.turns[0]),
    }),
  ).toThrow(/invalid_turns/);
  expect(() => parseTurnReceipt({ id: TURN, state: 'running' })).toThrow();
  expect(() => parseTurnReceipt({ id: 'nope' })).toThrow();
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

it('sends four narrow requests and no other, each as one typed request', async () => {
  const calls = bridge(({ command, payload }) => {
    if (command === 'managed_turns') return fixture.list;
    if (command !== 'managed_turn_read') return { id: TURN };
    // What follows the cursor that was asked from, as the desktop answers it.
    const after = (payload as { request: { after: string | null } }).request
      .after;
    return {
      ...fixture.view,
      events: fixture.view.events.filter(
        (event) => Number(event.sequence) > Number(after ?? 0),
      ),
    };
  });
  await readTurns(SESSION);
  await readTurn(TURN, null, true);
  await readTurn(TURN, '2', false);
  // What the user wrote goes whole, as one field. It is not split, wrapped or trimmed.
  const prompt = '  $(rm -rf ~); `id` && echo "x"\n--flag ; two lines  ';
  await startTurn('intent-1', SESSION, 1, prompt);
  await cancelTurn('intent-2', TURN, 2);
  expect(calls).toEqual([
    { command: 'managed_turns', payload: { request: { session_id: SESSION } } },
    {
      command: 'managed_turn_read',
      payload: { request: { turn_id: TURN, after: null, wait: true } },
    },
    {
      command: 'managed_turn_read',
      payload: { request: { turn_id: TURN, after: '2', wait: false } },
    },
    {
      command: 'managed_turn_start',
      payload: {
        request: {
          intent_id: 'intent-1',
          session_id: SESSION,
          expected_revision: 1,
          prompt,
        },
      },
    },
    {
      command: 'managed_turn_cancel',
      payload: {
        request: { intent_id: 'intent-2', turn_id: TURN, expected_revision: 2 },
      },
    },
  ]);
  // Nothing in a request names a control method, a scope, a lease, a runtime or a tool.
  expect(JSON.stringify(calls)).not.toMatch(
    /managedSession\/|method|scope|lease|ttl|runtime|thread|model|tool|sandbox|approv/,
  );
});

it('takes an answer about another turn, session or position for a failed bridge', async () => {
  const internal = { code: 'internal', retry: false };
  // Another session's list: well formed, and not an answer to what was asked.
  const theirs = tampered(fixture.empty, ['session', 'id'], OTHER);
  expect(() => parseTurnList(theirs)).not.toThrow();
  bridge(() => theirs);
  expect(await failure(readTurns(SESSION))).toEqual(internal);
  // Another turn's reading, and another turn's receipt for a cancel.
  bridge(({ command }) =>
    command === 'managed_turn_read'
      ? tampered(fixture.view, ['turn', 'id'], OTHER)
      : { id: OTHER },
  );
  expect(await failure(readTurn(TURN, null, false))).toEqual(internal);
  expect(await failure(cancelTurn('i', TURN, 2))).toEqual(internal);
  // Events at or before the cursor that was asked from: a repeat is not shown twice.
  bridge(() => fixture.view);
  expect(await failure(readTurn(TURN, '3', false))).toEqual(internal);
  expect(await failure(readTurn(TURN, '1', false))).toEqual(internal);
  // A cursor that went back, or went away.
  bridge(() => ({ ...fixture.view, events: [], next: '2' }));
  expect(await failure(readTurn(TURN, '4', true))).toEqual(internal);
  bridge(() => ({ ...fixture.view, events: [], next: null }));
  expect(await failure(readTurn(TURN, '4', true))).toEqual(internal);
  expect((await readTurn(TURN, null, true)).next).toBeNull();
  // Not the projection at all.
  bridge(() => ({ result: { type: 'turn_events', events: [] } }));
  expect(await failure(readTurn(TURN, null, false))).toEqual(internal);
  expect(await failure(readTurns(SESSION))).toEqual(internal);
  expect(await failure(startTurn('i', SESSION, 1, 'x'))).toEqual(internal);
});

it('reports a refusal by the name the desktop gave it, and anything else as internal', async () => {
  bridge(({ command }) => {
    throw command === 'managed_turn_start'
      ? { code: 'busy', retry: true }
      : command === 'managed_turn_cancel'
        ? { code: 'changed', retry: false }
        : command === 'managed_turns'
          ? { code: 'unavailable', retry: true, cause: 'timeout' }
          : 'managedSession/turn/events failed: codex thread 0199…';
  });
  expect(await failure(startTurn('i', SESSION, 1, 'x'))).toEqual({
    code: 'busy',
    retry: true,
  });
  expect(await failure(cancelTurn('i', TURN, 2))).toEqual({
    code: 'changed',
    retry: false,
  });
  // A failure that is not exactly one the desktop names carries nothing through.
  expect(await failure(readTurns(SESSION))).toEqual({
    code: 'internal',
    retry: false,
  });
  expect(await failure(readTurn(TURN, null, true))).toEqual({
    code: 'internal',
    retry: false,
  });
});

// ---- Older turns -----------------------------------------------------------------------

const CURSOR = fixture.list.older as unknown as HistoryCursor;

it('accepts a position in a stream and a page of older turns, and nothing beside them', () => {
  expect(parseHistoryCursor(CURSOR)).toEqual(CURSOR);
  expect(parseTurnHistory(fixture.history)).toEqual(fixture.history);
  expect(parseTurnHistory(fixture.history_end)).toEqual(fixture.history_end);
  // The list says where older turns begin, or that none do.
  expect(parseTurnList(fixture.list).older).toEqual(CURSOR);
  expect(parseTurnList(fixture.empty).older).toBeNull();

  const cursor = (value: unknown) => () => parseHistoryCursor(value);
  for (const bad of [
    { ...CURSOR, stream_id: 'x' },
    { ...CURSOR, stream_id: null },
    { ...CURSOR, before: 6 },
    { ...CURSOR, before: '0' },
    { ...CURSOR, before: '06' },
    { ...CURSOR, before: '-6' },
    { ...CURSOR, before: '' },
    // Position one is the first: nothing is before it, so nothing names it.
    { ...CURSOR, before: '1' },
    { ...CURSOR, extra: true },
    { stream_id: CURSOR.stream_id },
    { before: CURSOR.before },
    null,
    '6',
    [],
  ])
    expect(cursor(bad), JSON.stringify(bad)).toThrow(/invalid_cursor/);
});

it('refuses a list or a page that is not exactly the projection or that contradicts itself', () => {
  const list = (path: (string | number)[], value: unknown) => () =>
    parseTurnList(tampered(fixture.list, path, value));
  // The old shape is another version, and a missing field is not filled in.
  expect(list(['schema_version'], 2)).toThrow(/schema_version/);
  expect(list(['older'], undefined)).toThrow(/invalid_list/);
  expect(list(['older', 'before'], '1')).toThrow(/invalid_cursor/);
  // History that was not read whole always has somewhere older to look.
  expect(list(['partial'], true)).not.toThrow();
  expect(() =>
    parseTurnList({ ...fixture.empty, partial: true, older: null }),
  ).toThrow(/invalid_hints/);

  const page = (path: (string | number)[], value: unknown) => () =>
    parseTurnHistory(tampered(fixture.history, path, value));
  expect(page(['schema_version'], 2)).toThrow(/schema_version/);
  expect(page(['schema_version'], undefined)).toThrow(/invalid_history/);
  expect(page(['extra'], 1)).toThrow(/invalid_history/);
  expect(page(['older'], undefined)).toThrow(/invalid_history/);
  expect(page(['session'], 'nope')).toThrow();
  // Every turn is the session's, each is listed once, and none is still running.
  expect(page(['turns', 0, 'session'], OTHER)).toThrow(/invalid_turns/);
  expect(page(['turns', 1, 'id'], fixture.history.turns[0].id)).toThrow(
    /invalid_turns/,
  );
  expect(
    page(['turns', 0], {
      ...fixture.history.turns[0],
      state: 'running',
      ended_at: null,
      reason: null,
      cancellable: true,
    }),
  ).toThrow(/invalid_turns/);
  expect(page(['turns', 0, 'state'], 'paused')).toThrow();
  expect(page(['turns'], 'none')).toThrow(/invalid_turns/);
  expect(() =>
    parseTurnHistory({
      ...fixture.history,
      turns: Array.from({ length: 21 }, (_, i) => ({
        ...fixture.history.turns[0],
        id: `${String(i).padStart(8, '0')}-aaaa-4aaa-8aaa-aaaaaaaaaaaa`,
      })),
    }),
  ).toThrow(/invalid_turns/);
  expect(page(['older', 'stream_id'], 'x')).toThrow(/invalid_cursor/);
});

it('asks for older turns with a session and the position it was handed, and nothing else', async () => {
  const calls = bridge(() => fixture.history);
  const page = await readOlderTurns(SESSION, CURSOR);
  expect(page).toEqual(fixture.history);
  expect(calls).toEqual([
    {
      command: 'managed_turns_older',
      payload: {
        request: {
          session_id: SESSION,
          stream_id: CURSOR.stream_id,
          before: CURSOR.before,
        },
      },
    },
  ]);
});

it('does not take an answer about another session, or one that does not go back', async () => {
  const answer = (over: object) =>
    bridge(() => ({ ...fixture.history, ...over }));
  const refused = async (over: object) => {
    answer(over);
    return failure(readOlderTurns(SESSION, CURSOR));
  };
  // Another session's turns are not the answer to this session's question.
  expect(await refused({ session: OTHER })).toEqual({
    code: 'internal',
    retry: false,
  });
  // History only goes back, in the stream it was asked in.
  const next = (before: string, stream: string = CURSOR.stream_id) => ({
    older: { stream_id: stream, before },
  });
  for (const over of [
    next(CURSOR.before),
    next('7'),
    next('99999'),
    next('3', '77777777-7777-4777-8777-777777777777'),
  ])
    expect(await refused(over), JSON.stringify(over)).toEqual({
      code: 'internal',
      retry: false,
    });
  clearMocks();
  bridge(() => ({ ...fixture.history, ...next('5') }));
  expect((await readOlderTurns(SESSION, CURSOR)).older?.before).toBe('5');
  // An answer that is not the projection is a failed bridge, never data.
  clearMocks();
  bridge(() => ({ ...fixture.history, schema_version: 3 }));
  expect(await failure(readOlderTurns(SESSION, CURSOR))).toEqual({
    code: 'internal',
    retry: false,
  });
  // What the desktop refuses with is passed on as it is.
  clearMocks();
  bridge(() => Promise.reject({ code: 'changed', retry: false }));
  expect(await failure(readOlderTurns(SESSION, CURSOR))).toEqual({
    code: 'changed',
    retry: false,
  });
});
