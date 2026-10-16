import { afterEach, expect, it, vi } from 'vitest';
import { MAX_HISTORY_TURNS } from '../src/lib/features/agent/controller';
import { parseManagedTurnId } from '../src/lib/contracts/identities';
import type { TurnSummary } from '../src/lib/contracts/turns';
import {
  cursor,
  historyPage,
  listing,
  reading,
  S,
  S2,
  showing,
  turn,
} from './agent-harness';
import { connected, offline, settle } from './sessions-harness';

afterEach(() => vi.useRealTimers());

const OTHER_STREAM = '77777777-7777-4777-8777-777777777777';
/** The turn the authority numbered `n`: later turns have larger numbers. */
const tid = (n: number) =>
  parseManagedTurnId(
    `${n.toString(16).padStart(8, '0')}-0000-4000-8000-000000000000`,
  );
/** An ended turn whose output the authority no longer holds, as an old one is. */
const old = (n: number, over: Partial<TurnSummary> = {}) =>
  turn(tid(n), 'completed', 3, { output: false, ...over });
/** Turns `high` down to `low`, newest first, as the authority lists and pages them. */
const span = (high: number, low: number, over: Partial<TurnSummary> = {}) =>
  Array.from({ length: high - low + 1 }, (_, i) => old(high - i, over));
const numbers = (h: Awaited<ReturnType<typeof open>>) =>
  h.shown().turns.map((t) => parseInt(t.id.slice(0, 8), 16));
const range = (from: number, to: number) =>
  Array.from({ length: to - from + 1 }, (_, i) => from + i);

/** `S` shown with its newest twenty turns, 81 to 100, and older ones beyond. */
async function open() {
  return showing(span(100, 81), { older: cursor(500) });
}
/** The ask for older turns in flight, answered with `turns` going on from `older`. */
async function answer(
  h: Awaited<ReturnType<typeof open>>,
  turns: TurnSummary[],
  older: ReturnType<typeof cursor> | null,
) {
  h.olders.at(-1)!.resolve(historyPage(turns, older));
  await settle();
}

it('asks for older turns from the position the list gave and shows them above the newest', async () => {
  const h = await open();
  expect(h.shown().history).toEqual({
    more: true,
    status: 'idle',
    failure: null,
    searched: false,
    reachedStart: false,
    loaded: 0,
    capped: false,
  });
  expect(h.olders).toHaveLength(0);

  void h.controller.loadOlder();
  expect(h.olders).toHaveLength(1);
  expect(h.olders[0].session).toBe(S);
  expect(h.olders[0].cursor).toEqual(cursor(500));
  expect(h.shown().history.status).toBe('loading');
  // Asked again while it is asked: still one question.
  void h.controller.loadOlder();
  void h.controller.loadOlder();
  expect(h.olders).toHaveLength(1);

  await answer(h, span(80, 61), cursor(300));
  // Oldest first, the older page above the newest, no turn twice.
  expect(numbers(h)).toEqual(range(61, 100));
  expect(h.shown().history).toMatchObject({
    more: true,
    status: 'idle',
    loaded: 20,
    searched: false,
    reachedStart: false,
  });
  // Nothing is made up for what the authority no longer holds.
  const first = h.shown().turns[0];
  expect(first.items).toEqual([]);
  expect(first.turn?.output).toBe(false);
  expect(first.prompt).toBeNull();

  // The next page goes on from where the last one said, and the last one ends history.
  void h.controller.loadOlder();
  expect(h.olders[1].cursor).toEqual(cursor(300));
  await answer(h, span(60, 41), null);
  expect(numbers(h)).toEqual(range(41, 100));
  expect(h.shown().history).toMatchObject({
    more: false,
    reachedStart: true,
    loaded: 40,
  });
  // Nothing more to ask for.
  const asked = h.olders.length;
  await h.controller.loadOlder();
  expect(h.olders).toHaveLength(asked);
});

it('adds no turn that is already shown, whichever page repeats it', async () => {
  const h = await open();
  void h.controller.loadOlder();
  // A page that overlaps the newest ones and itself.
  await answer(h, [...span(85, 81), ...span(80, 71), old(75)], cursor(300));
  expect(numbers(h)).toEqual(range(71, 100));
  void h.controller.loadOlder();
  await answer(h, span(72, 60), cursor(200));
  expect(numbers(h)).toEqual(range(60, 100));
  const ids = h.shown().turns.map((t) => t.id);
  expect(new Set(ids).size).toBe(ids.length);
});

it('keeps what is shown when an ask fails and asks again from the same position', async () => {
  const h = await open();
  void h.controller.loadOlder();
  h.olders[0].reject({ code: 'unavailable', retry: true });
  await settle();
  expect(h.shown().history).toMatchObject({
    status: 'failed',
    failure: { code: 'unavailable', retry: true },
    more: true,
  });
  expect(numbers(h)).toEqual(range(81, 100));

  void h.controller.loadOlder();
  expect(h.olders).toHaveLength(2);
  expect(h.olders[1].cursor).toEqual(cursor(500));
  expect(h.shown().history.failure).toBeNull();
  expect(h.shown().history.status).toBe('loading');
  await answer(h, span(80, 61), cursor(300));
  expect(h.shown().history).toMatchObject({ status: 'idle', loaded: 20 });
});

it('lets go of a position the authority calls history that changed, and reads the newest again', async () => {
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, span(80, 61), cursor(300));
  void h.controller.loadOlder();
  h.olders[1].reject({ code: 'changed', retry: false });
  await settle();
  expect(h.shown().history).toMatchObject({
    status: 'failed',
    failure: { code: 'changed' },
  });
  // What was loaded stays shown until the reader reloads.
  expect(numbers(h)).toEqual(range(61, 100));

  const lists = h.lists.length;
  h.controller.restartHistory();
  expect(h.lists).toHaveLength(lists + 1);
  expect(h.shown().history).toMatchObject({
    more: false,
    status: 'idle',
    failure: null,
    loaded: 0,
  });
  expect(numbers(h)).toEqual(range(81, 100));
  // The newest are read again, and say where the history they belong to begins.
  h.lists
    .at(-1)!
    .resolve(listing(span(100, 81), { older: cursor(900, OTHER_STREAM) }));
  await settle();
  void h.controller.loadOlder();
  expect(h.olders.at(-1)!.cursor).toEqual(cursor(900, OTHER_STREAM));
  await answer(h, span(80, 71), cursor(600, OTHER_STREAM));
  expect(numbers(h)).toEqual(range(71, 100));
});

it('refreshing after a stale position starts the history over, and refreshing otherwise does not', async () => {
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, span(80, 61), cursor(300));
  // An ordinary refresh keeps the older turns and the position.
  h.controller.refresh();
  h.lists.at(-1)!.resolve(listing(span(100, 81), { older: cursor(500) }));
  await settle();
  expect(numbers(h)).toEqual(range(61, 100));
  expect(h.shown().history.loaded).toBe(20);
  // After a stale position, refreshing is the way out.
  void h.controller.loadOlder();
  h.olders[1].reject({ code: 'changed', retry: false });
  await settle();
  h.controller.refresh();
  h.lists.at(-1)!.resolve(listing(span(100, 81), { older: cursor(500) }));
  await settle();
  expect(numbers(h)).toEqual(range(81, 100));
  expect(h.shown().history).toMatchObject({ failure: null, loaded: 0 });
});

it('does not take an answer that arrives after the connection, the tab or the position is gone', async () => {
  // The connection is lost while an ask is in flight.
  const lost = await open();
  void lost.controller.loadOlder();
  lost.controller.observe(offline);
  // Not left asking: the reader sees a view they can act on once it is back.
  expect(lost.shown().history.status).toBe('idle');
  await answer(lost, span(80, 61), cursor(300));
  expect(numbers(lost)).toEqual(range(81, 100));
  expect(lost.shown().history.loaded).toBe(0);
  lost.controller.observe(connected('9'));
  lost.lists.at(-1)!.resolve(listing(span(100, 81), { older: cursor(500) }));
  await settle();
  void lost.controller.loadOlder();
  expect(lost.olders).toHaveLength(2);
  // Not asked at all while there is no connection.
  lost.controller.observe(offline);
  await lost.controller.loadOlder();
  expect(lost.olders).toHaveLength(2);

  // The tab is closed.
  const closed = await open();
  void closed.controller.loadOlder();
  closed.controller.close(S);
  await answer(closed, span(80, 61), cursor(300));
  expect(closed.state().current).toBeNull();
  expect(closed.state().tabs).toEqual([]);

  // The reader starts over while an ask is in flight.
  const restarted = await open();
  void restarted.controller.loadOlder();
  restarted.controller.restartHistory();
  await answer(restarted, span(80, 61), cursor(300));
  expect(numbers(restarted)).toEqual(range(81, 100));
  expect(restarted.shown().history.status).toBe('idle');
  expect(restarted.shown().history.loaded).toBe(0);
});

it('keeps older turns, in order and without a hole, while the newest page moves on', async () => {
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, span(80, 61), cursor(300));
  // A new turn is admitted: the newest page now begins one later, and turn 81, which was
  // listed, is no longer in it.
  h.controller.observe(connected('10'));
  h.lists
    .at(-1)!
    .resolve(listing(span(101, 82), { older: cursor(520), head: '10' }));
  await settle();
  // 81 is kept: the older turns end where it begins, and nothing is missing between.
  expect(numbers(h)).toEqual(range(61, 101));
  expect(h.shown().history.loaded).toBe(21);
  // The position held is the deeper one, not the one the new list gave.
  void h.controller.loadOlder();
  expect(h.olders.at(-1)!.cursor).toEqual(cursor(300));
  await answer(h, span(60, 51), cursor(250));
  expect(numbers(h)).toEqual(range(51, 101));
});

it('lets go of the older turns when the history a list belongs to is another', async () => {
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, span(80, 61), cursor(300));
  h.controller.observe(connected('10'));
  h.lists
    .at(-1)!
    .resolve(
      listing(span(100, 81), { older: cursor(40, OTHER_STREAM), head: '10' }),
    );
  await settle();
  expect(numbers(h)).toEqual(range(81, 100));
  expect(h.shown().history).toMatchObject({ loaded: 0, more: true });
  void h.controller.loadOlder();
  expect(h.olders.at(-1)!.cursor).toEqual(cursor(40, OTHER_STREAM));
});

it('says when a stretch of history held no turn, and goes on from where it stopped', async () => {
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, [], cursor(250));
  expect(h.shown().history).toMatchObject({
    searched: true,
    more: true,
    reachedStart: false,
    loaded: 0,
  });
  expect(numbers(h)).toEqual(range(81, 100));
  void h.controller.loadOlder();
  expect(h.olders[1].cursor).toEqual(cursor(250));
  await answer(h, span(40, 31), null);
  expect(h.shown().history).toMatchObject({
    searched: false,
    more: false,
    reachedStart: true,
  });
  // Nothing at all older: said once the end is reached, never before.
  const none = await open();
  void none.controller.loadOlder();
  await answer(none, [], null);
  expect(none.shown().history).toMatchObject({
    more: false,
    reachedStart: true,
    loaded: 0,
    searched: false,
  });
});

it('offers nothing when the list says the whole history is shown', async () => {
  const h = await showing(span(3, 1), { older: null });
  expect(h.shown().history).toMatchObject({
    more: false,
    reachedStart: false,
    loaded: 0,
  });
  await h.controller.loadOlder();
  expect(h.olders).toHaveLength(0);
});

it('stops at the bound the window keeps, lets the oldest go beyond it, and starts over on request', async () => {
  // Numbered high enough to page back through the whole bound.
  const top = 2000;
  const h = await showing(span(top, top - 19), { older: cursor(9000) });
  let low = top - 19;
  for (let page = 0; page < MAX_HISTORY_TURNS / 20; page++) {
    void h.controller.loadOlder();
    await answer(h, span(low - 1, low - 20), cursor(8000 - page * 20));
    low -= 20;
  }
  expect(h.shown().history).toMatchObject({
    loaded: MAX_HISTORY_TURNS,
    capped: true,
    more: false,
  });
  expect(h.shown().turns).toHaveLength(MAX_HISTORY_TURNS + 20);
  const asked = h.olders.length;
  await h.controller.loadOlder();
  expect(h.olders).toHaveLength(asked);

  // A new turn pushes one more out of the newest page: the oldest older turn lets go, so
  // what is shown stays within the bound and has no hole in it.
  h.controller.observe(connected('10'));
  h.lists
    .at(-1)!
    .resolve(
      listing(span(top + 1, top - 18), { older: cursor(9100), head: '10' }),
    );
  await settle();
  expect(h.shown().history.loaded).toBe(MAX_HISTORY_TURNS);
  expect(numbers(h)).toEqual(range(low + 1, top + 1));
  expect(h.shown().history.capped).toBe(true);

  h.controller.restartHistory();
  h.lists
    .at(-1)!
    .resolve(listing(span(top + 1, top - 18), { older: cursor(9100) }));
  await settle();
  expect(numbers(h)).toEqual(range(top - 18, top + 1));
  expect(h.shown().history).toMatchObject({
    capped: false,
    more: true,
    loaded: 0,
  });
});

it('does not hold up the turn that is running, and reads old output only when nothing runs', async () => {
  const running = turn(tid(100), 'running', 2);
  const h = await showing([running, ...span(99, 81)], { older: cursor(500) });
  // The running turn is the one followed, from the first moment.
  expect(h.reads.map((r) => [r.turn, r.wait])).toEqual([[tid(100), true]]);

  void h.controller.loadOlder();
  expect(h.olders).toHaveLength(1);
  // Asking for history took nothing from following: still the one reading, still waiting.
  expect(h.reads).toHaveLength(1);
  await answer(h, [old(50, { output: true }), old(49)], cursor(300));
  expect(numbers(h).slice(0, 2)).toEqual([49, 50]);
  // The older turn still holds output, and it is not read while the running turn is.
  expect(h.reads).toHaveLength(1);
  expect(h.flight.most).toBe(1);

  // The running turn put something out and goes on. What is read next is that turn again,
  // from where it got to, not the older turn that is waiting for its output to be read.
  h.reads[0].resolve(
    reading(turn(tid(100), 'running', 2), [
      [1, 'started'],
      [2, 'text', 'Still '],
    ]),
  );
  await settle();
  expect(h.reads.map((r) => [r.turn, r.after, r.wait]).at(-1)).toEqual([
    tid(100),
    '2',
    true,
  ]);
  expect(h.reads).toHaveLength(2);

  h.reads[1].resolve(
    reading(turn(tid(100), 'completed', 3), [
      [3, 'text', 'Done.'],
      [4, 'completed'],
    ]),
  );
  await settle();
  // Now that nothing runs, what is held of the older turn is read, once, not waiting.
  expect(h.reads.map((r) => [r.turn, r.after, r.wait]).at(-1)).toEqual([
    tid(50),
    null,
    false,
  ]);
  h.reads.at(-1)!.resolve(
    reading(old(50, { output: true }), [
      [1, 'started'],
      [2, 'text', 'Long ago.'],
      [3, 'completed'],
    ]),
  );
  await settle();
  const entry = h.shown().turns.find((t) => t.id === tid(50))!;
  expect(entry.items.map((i) => (i.type === 'text' ? i.text : i.type))).toEqual(
    ['Long ago.'],
  );
  expect(h.flight.most).toBe(1);
});

it('asks for nothing while idle: older turns are read only when the reader asks', async () => {
  vi.useFakeTimers();
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, span(80, 61), cursor(300));
  const calls = h.calls() + h.olders.length;
  await vi.advanceTimersByTimeAsync(120_000);
  h.controller.observe(connected('9'));
  await vi.advanceTimersByTimeAsync(120_000);
  expect(h.calls() + h.olders.length).toBe(calls);
});

it('keeps the older turns of each open session apart', async () => {
  const h = await open();
  void h.controller.loadOlder();
  await answer(h, span(80, 61), cursor(300));
  // Another session, with nothing older, is opened and shown.
  h.controller.open(S2, 'other');
  h.lists
    .at(-1)!
    .resolve(
      listing([], { session: { id: S2, workspace: '/home/dev/other' } }),
    );
  await settle();
  expect(h.shown().session).toBe(S2);
  expect(h.shown().turns).toEqual([]);
  expect(h.shown().history).toMatchObject({ more: false, loaded: 0 });
  await h.controller.loadOlder();
  expect(h.olders).toHaveLength(1);
  // The first session still has what it loaded and where it was.
  h.controller.select(S);
  expect(numbers(h)).toEqual(range(61, 100));
  void h.controller.loadOlder();
  expect(h.olders[1]).toMatchObject({ session: S, cursor: cursor(300) });
});

it('shows a list of a session that has no older turns as it always did', async () => {
  const h = await showing(span(100, 81), { older: null });
  expect(numbers(h)).toEqual(range(81, 100));
  // Twenty-first turn: the oldest slides out, and is kept rather than dropped.
  h.controller.observe(connected('10'));
  h.lists
    .at(-1)!
    .resolve(listing(span(101, 82), { older: cursor(60), head: '10' }));
  await settle();
  expect(numbers(h)).toEqual(range(81, 101));
  expect(h.shown().history).toMatchObject({ more: false, loaded: 1 });
});
