import { afterEach, expect, it, vi } from 'vitest';
import type { EventsPage } from '../src/lib/contracts/events';
import type { SessionFailure } from '../src/lib/contracts/sessions';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import {
  EventsController,
  MAX_EVENTS,
  MAX_PREVIOUS,
  type EventsClient,
  type EventsState,
} from '../src/lib/features/events/controller';
import {
  eventClock,
  eventDay,
  shownEvents,
} from '../src/lib/features/events/present';
import {
  OTHER_STREAM,
  page,
  positions,
  run,
  S1,
  S2,
  STREAM,
  T1,
  T2,
} from './events-harness';
import { connected, offline, settle } from './sessions-harness';

afterEach(() => vi.useRealTimers());

const UNAVAILABLE = { code: 'unavailable', retry: true } as const;
interface Pending {
  resolve: (page: EventsPage) => void;
  reject: (failure: SessionFailure) => void;
}
function harness() {
  const latest: Pending[] = [];
  const older: (Pending & { stream: string; before: string })[] = [];
  const deferred = () => {
    let resolve!: (page: EventsPage) => void;
    let fail!: (error: unknown) => void;
    const promise = new Promise<EventsPage>((res, rej) => {
      resolve = res;
      fail = rej;
    });
    return {
      promise,
      resolve,
      reject: (failure: SessionFailure) => fail(new SessionCallError(failure)),
    };
  };
  const client: EventsClient = {
    latest() {
      const d = deferred();
      latest.push(d);
      return d.promise;
    },
    older(stream, before) {
      const d = deferred();
      older.push({ ...d, stream, before });
      return d.promise;
    },
  };
  let state!: EventsState;
  let published = 0;
  const controller = new EventsController((next) => {
    state = next;
    published++;
  }, client);
  state = controller.state();
  return {
    controller,
    client,
    latest,
    older,
    state: () => state,
    published: () => published,
    calls: () => latest.length + older.length,
  };
}
/** The Events view shown and connected at `head`, its first window answered. */
async function showing(head = 120, count = 50) {
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected(String(head)));
  h.latest[0].resolve(page(head, count));
  await settle();
  return h;
}

it('reads history only while it is shown, once for each change, with no timer', async () => {
  vi.useFakeTimers();
  const h = harness();
  // Connected and hidden: whatever happens at the authority, nothing is read.
  h.controller.observe(connected('100'));
  h.controller.observe(connected('110'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(0);
  expect(h.state()).toMatchObject({ loaded: false, events: [] });
  // Shown: the newest window, once.
  h.controller.setVisible(true);
  expect(h.latest).toHaveLength(1);
  expect(h.state().reading).toBe(true);
  h.controller.observe(connected('110'));
  h.controller.setVisible(true);
  expect(h.latest).toHaveLength(1);
  h.latest[0].resolve(page(110, 50));
  await settle();
  expect(h.state()).toMatchObject({
    loaded: true,
    reading: false,
    stream: STREAM,
    start: 'more',
    newer: false,
  });
  expect(positions(h.state().events)).toEqual(run(110, 61));
  // Nothing changed: a minute of nothing, and no timer was ever set.
  h.controller.observe(connected('110'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(1);
  expect(vi.getTimerCount()).toBe(0);
  // The durable head moved while the newest end is shown: read once, and added.
  h.controller.observe(connected('113'));
  h.controller.observe(connected('113'));
  expect(h.latest).toHaveLength(2);
  h.latest[1].resolve(page(113, 50));
  await settle();
  expect(positions(h.state().events)).toEqual(run(113, 61));
  // Hidden again: a change is not read until the view is shown.
  h.controller.setVisible(false);
  h.controller.observe(connected('120'));
  h.controller.observe(connected('125'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(2);
  h.controller.setVisible(true);
  expect(h.latest).toHaveLength(3);
  expect(vi.getTimerCount()).toBe(0);
});

it('adds new events once, in order, and never an event it already shows', async () => {
  const h = await showing(120, 50);
  // The next window overlaps what is shown by forty-seven events.
  h.controller.observe(connected('123'));
  h.latest[1].resolve(page(123, 50));
  await settle();
  const shown = positions(h.state().events);
  expect(shown).toEqual(run(123, 71));
  expect(new Set(shown).size).toBe(shown.length);
  // The same window again, as when two changes are read one after the other.
  h.controller.refresh();
  h.latest[2].resolve(page(123, 50));
  await settle();
  expect(positions(h.state().events)).toEqual(run(123, 71));
  // More arrived than a window holds: what is shown is the newest window, whole, and
  // what lay between is not claimed.
  h.controller.observe(connected('400'));
  h.latest[3].resolve(page(400, 50));
  await settle();
  expect(positions(h.state().events)).toEqual(run(400, 351));
  expect(h.state()).toMatchObject({ start: 'more', detached: false });
});

it('tells positions apart exactly, past what a number holds', async () => {
  // 2^53 and the position after it are the same number, and not the same position.
  const h = await showing(9007199254740992, 3);
  expect(positions(h.state().events)[0]).toBe('9007199254740992');
  h.controller.observe(connected('9007199254740993'));
  expect(h.latest).toHaveLength(2);
  // And a head that did not move is not read again.
  const still = await showing(9007199254740992, 3);
  still.controller.observe(connected('9007199254740992'));
  expect(still.latest).toHaveLength(1);
});

it('does not move history under a reader who is somewhere older', async () => {
  const h = await showing(120, 50);
  h.controller.setAtTop(false);
  // New events arrive. Nothing is read into the view; the reader is told.
  h.controller.observe(connected('125'));
  h.controller.observe(connected('130'));
  await settle();
  expect(h.latest).toHaveLength(1);
  expect(h.state().newer).toBe(true);
  expect(positions(h.state().events)).toEqual(run(120, 71));
  // Back at the newest end: now they are read and added.
  h.controller.setAtTop(true);
  expect(h.latest).toHaveLength(2);
  h.latest[1].resolve(page(130, 50));
  await settle();
  expect(h.state().newer).toBe(false);
  expect(positions(h.state().events)).toEqual(run(130, 71));
  // Asking for the newest from older history is the way back: the newest window.
  h.controller.setAtTop(false);
  h.controller.observe(connected('140'));
  expect(h.state().newer).toBe(true);
  h.controller.refresh();
  h.latest[2].resolve(page(140, 50));
  await settle();
  expect(positions(h.state().events)).toEqual(run(140, 91));
  expect(h.state().newer).toBe(false);
});

it('reads older history a window at a time, from the position it was handed', async () => {
  const h = await showing(120, 50);
  h.controller.loadOlder();
  h.controller.loadOlder();
  // One request, with the position as the text the last window handed back.
  expect(h.older).toHaveLength(1);
  expect(h.older[0]).toMatchObject({ stream: STREAM, before: '71' });
  expect(h.state().loadingOlder).toBe(true);
  h.older[0].resolve(page(70, 50, { head: '120' }));
  await settle();
  expect(positions(h.state().events)).toEqual(run(120, 21));
  expect(h.state()).toMatchObject({ loadingOlder: false, start: 'more' });
  h.controller.loadOlder();
  expect(h.older[1]).toMatchObject({ before: '21' });
  h.older[1].resolve(page(20, 20, { head: '120' }));
  await settle();
  // The beginning of the stream: said, and nothing more is asked for.
  expect(positions(h.state().events)).toEqual(run(120, 1));
  expect(new Set(positions(h.state().events)).size).toBe(120);
  expect(h.state().start).toBe('beginning');
  h.controller.loadOlder();
  expect(h.older).toHaveLength(2);

  // An answer that overlaps what is shown adds each event once.
  const overlap = await showing(120, 50);
  overlap.controller.loadOlder();
  overlap.older[0].resolve(page(75, 50, { head: '120' }));
  await settle();
  expect(positions(overlap.state().events)).toEqual(run(120, 26));

  // A window that begins at a hole: history before it is unavailable, and that is all.
  const gap = await showing(120, 50);
  gap.controller.loadOlder();
  gap.older[0].resolve(page(70, 30, { head: '120', gap: true }));
  await settle();
  expect(positions(gap.state().events)).toEqual(run(120, 41));
  expect(gap.state().start).toBe('unavailable');
  gap.controller.loadOlder();
  expect(gap.older).toHaveLength(1);
  // None of the window before is there: the same is said, and what is shown stays.
  const gone = await showing(120, 50);
  gone.controller.loadOlder();
  gone.older[0].resolve(page(0, 0, { head: '120', gap: true }));
  await settle();
  expect(positions(gone.state().events)).toEqual(run(120, 71));
  expect(gone.state()).toMatchObject({
    start: 'unavailable',
    loadingOlder: false,
    olderFailure: null,
  });
  gone.controller.loadOlder();
  expect(gone.older).toHaveLength(1);
  // The newest window itself may begin at one.
  const short = harness();
  short.controller.setVisible(true);
  short.controller.observe(connected('120'));
  short.latest[0].resolve(page(120, 10, { gap: true }));
  await settle();
  expect(short.state().start).toBe('unavailable');

  // A window that could not be read: said, and what is shown stays.
  const failed = await showing(120, 50);
  failed.controller.loadOlder();
  failed.older[0].reject(UNAVAILABLE);
  await settle();
  expect(failed.state()).toMatchObject({
    olderFailure: UNAVAILABLE,
    loadingOlder: false,
    start: 'more',
  });
  expect(failed.latest).toHaveLength(1);
  // The authority says the position belongs to another history: where it is now is
  // read, once.
  failed.controller.loadOlder();
  failed.older[1].reject({ code: 'changed', retry: false });
  await settle();
  expect(failed.latest).toHaveLength(2);
});

it('keeps a bounded number of events, whichever way the reader goes', async () => {
  const h = await showing(1000, 50);
  let oldest = 951;
  // Seven more windows: four hundred events, the bound.
  for (let i = 0; i < 7; i++) {
    h.controller.loadOlder();
    h.older.at(-1)!.resolve(page(oldest - 1, 50, { head: '1000' }));
    await settle();
    oldest -= 50;
  }
  expect(h.state().events).toHaveLength(MAX_EVENTS);
  expect(h.state().detached).toBe(false);
  // One more: room is made at the newest end, and the view says it is no longer there.
  h.controller.loadOlder();
  h.older.at(-1)!.resolve(page(oldest - 1, 50, { head: '1000' }));
  await settle();
  expect(h.state().events).toHaveLength(MAX_EVENTS);
  expect(positions(h.state().events)).toEqual(run(950, 551));
  expect(h.state().detached).toBe(true);
  // Detached, new events are announced, never spliced on top of older history.
  h.controller.observe(connected('1010'));
  await settle();
  expect(h.latest).toHaveLength(1);
  expect(h.state().newer).toBe(true);
  h.controller.refresh();
  h.latest[1].resolve(page(1010, 50));
  await settle();
  expect(positions(h.state().events)).toEqual(run(1010, 961));
  expect(h.state()).toMatchObject({ detached: false, newer: false });

  // At the newest end the far end is what is let go, and the position to read older
  // from moves with it.
  const top = await showing(400, 50);
  for (let i = 0, last = 351; i < 7; i++, last -= 50) {
    top.controller.loadOlder();
    top.older.at(-1)!.resolve(page(last - 1, 50, { head: '400' }));
    await settle();
  }
  expect(positions(top.state().events)).toEqual(run(400, 1));
  expect(top.state().start).toBe('beginning');
  top.controller.observe(connected('430'));
  top.latest[1].resolve(page(430, 50));
  await settle();
  expect(top.state().events).toHaveLength(MAX_EVENTS);
  expect(positions(top.state().events)).toEqual(run(430, 31));
  expect(top.state().start).toBe('more');
  top.controller.loadOlder();
  expect(top.older.at(-1)).toMatchObject({ before: '31' });
});

it('never takes the events of another stream for a continuation of this one', async () => {
  const h = await showing(150, 50);
  // A hundred and fifty events of this stream are shown.
  for (const last of [101, 51]) {
    h.controller.loadOlder();
    h.older.at(-1)!.resolve(page(last - 1, 50, { head: '150' }));
    await settle();
  }
  expect(h.state().events).toHaveLength(150);
  h.controller.select({ stream: STREAM, sequence: '150' });
  // The authority now holds another stream, whose positions start over.
  h.controller.observe(connected('3', { restarts: 1 }));
  h.latest[1].resolve(page(3, 3, { stream: OTHER_STREAM }));
  await settle();
  const state = h.state();
  expect(state.stream).toBe(OTHER_STREAM);
  expect(positions(state.events)).toEqual(['3', '2', '1']);
  // What was shown is kept apart, bounded, as events of the earlier stream.
  expect(state.previous?.stream).toBe(STREAM);
  expect(positions(state.previous!.events)).toEqual(run(150, 51));
  expect(state.previous!.events).toHaveLength(MAX_PREVIOUS);
  expect(state.start).toBe('beginning');
  // The selection is still the event it was, in the stream it belongs to.
  expect(state.selected).toEqual({ stream: STREAM, sequence: '150' });
  // Position 3 of the new stream is not position 3 of anything else: new events with
  // positions the old stream also had are all shown.
  h.controller.observe(connected('103', { restarts: 1 }));
  h.latest[2].resolve(page(103, 50, { stream: OTHER_STREAM }));
  await settle();
  expect(positions(h.state().events)).toEqual(run(103, 54));
  expect(positions(h.state().previous!.events)).toEqual(run(150, 51));
  // Older history is asked for in the stream that is shown, from its own position.
  h.controller.loadOlder();
  expect(h.older.at(-1)).toMatchObject({ stream: OTHER_STREAM, before: '54' });

  // A window of the old stream that answers after the stream changed is not appended.
  const late = await showing(150, 50);
  late.controller.loadOlder();
  late.controller.observe(connected('2', { restarts: 1 }));
  await settle();
  // The stream is read, not assumed: with a read in flight the older answer waits.
  late.latest[1].resolve(page(2, 2, { stream: OTHER_STREAM }));
  await settle();
  late.older[0].resolve(page(100, 50, { head: '150' }));
  await settle();
  expect(positions(late.state().events)).toEqual(['2', '1']);
  expect(late.state().loadingOlder).toBe(false);
});

it('keeps what was read when the authority goes away, and says it is from before', async () => {
  const h = await showing(120, 50);
  h.controller.loadOlder();
  h.controller.observe(offline);
  expect(h.state()).toMatchObject({
    link: 'offline',
    stale: true,
    loadingOlder: false,
    reading: false,
  });
  expect(positions(h.state().events)).toEqual(run(120, 71));
  // Nothing is read while it is away, whatever is asked.
  const calls = h.calls();
  h.controller.refresh();
  h.controller.loadOlder();
  h.controller.setAtTop(false);
  h.controller.setAtTop(true);
  expect(h.calls()).toBe(calls);
  // An answer from the connection that is gone is not shown.
  h.older[0].resolve(page(70, 50, { head: '120' }));
  await settle();
  expect(positions(h.state().events)).toEqual(run(120, 71));
  // Back, with the same head: it is read once all the same. Only a read says whether
  // this is the same history.
  h.controller.observe(connected('120'));
  expect(h.latest).toHaveLength(2);
  expect(h.state().stale).toBe(true);
  h.latest[1].resolve(page(120, 50));
  await settle();
  expect(h.state()).toMatchObject({ stale: false, link: 'connected' });
  expect(positions(h.state().events)).toEqual(run(120, 71));
  h.controller.observe(connected('120'));
  expect(h.latest).toHaveLength(2);

  // Never connected: nothing is shown and nothing is called stale.
  const cold = harness();
  cold.controller.setVisible(true);
  cold.controller.observe(offline);
  expect(cold.state()).toMatchObject({ loaded: false, stale: false });
  expect(cold.calls()).toBe(0);
});

it('says that the authority restarted, and reads where its history is', async () => {
  const h = await showing(120, 50);
  // Another instance answers on the same socket, with the same stream.
  h.controller.observe(connected('124', { restarts: 1 }));
  expect(h.state().restarted).toBe(true);
  expect(h.latest).toHaveLength(2);
  h.latest[1].resolve(page(124, 50));
  await settle();
  // The same stream: history continues, with no boundary.
  expect(h.state()).toMatchObject({ previous: null, restarted: true });
  expect(positions(h.state().events)).toEqual(run(124, 71));
  h.controller.acknowledge();
  expect(h.state().restarted).toBe(false);
  // A restart nobody was showing events for is not announced.
  const idle = harness();
  idle.controller.observe(connected('5'));
  idle.controller.observe(connected('6', { restarts: 1 }));
  expect(idle.state().restarted).toBe(false);
});

it('does not repeat a read that failed until something changes', async () => {
  vi.useFakeTimers();
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected('120'));
  h.latest[0].reject(UNAVAILABLE);
  await settle();
  expect(h.state()).toMatchObject({ loaded: false, readFailure: UNAVAILABLE });
  h.controller.observe(connected('120'));
  h.controller.setVisible(false);
  h.controller.setVisible(true);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.latest).toHaveLength(1);
  // The reader asks again.
  h.controller.refresh();
  expect(h.latest).toHaveLength(2);
  h.latest[1].resolve(page(120, 50));
  await settle();
  expect(h.state()).toMatchObject({ loaded: true, readFailure: null });
  // A later read fails with events shown: they stay, the failure is said, no loop.
  h.controller.observe(connected('121'));
  h.latest[2].reject(UNAVAILABLE);
  await settle();
  expect(h.state().readFailure).toEqual(UNAVAILABLE);
  expect(positions(h.state().events)).toEqual(run(120, 71));
  h.controller.observe(connected('121'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.latest).toHaveLength(3);
  // The head moves again: that is a change.
  h.controller.observe(connected('122'));
  expect(h.latest).toHaveLength(4);
});

it('filters what is loaded, and only that', async () => {
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected('6'));
  const kinds = [
    'session_prepared',
    'runtime_ready',
    'turn_running',
    'approval_requested',
    'lease_granted',
    'other',
  ] as const;
  h.latest[0].resolve(page(6, 6, { kind: (at) => kinds[at - 1] }));
  await settle();
  const shown = (filter: Parameters<typeof shownEvents>[1]) =>
    positions(shownEvents(h.state().events, filter, null));
  expect(shown('all')).toEqual(run(6, 1));
  expect(shown('sessions')).toEqual(['1']);
  expect(shown('runtime')).toEqual(['2']);
  expect(shown('turns')).toEqual(['3']);
  expect(shown('approvals')).toEqual(['4']);
  // Security is what the record marks as such: here, the lease.
  expect(shown('security')).toEqual(['5']);
  // A filter is the view's own: choosing one reads nothing.
  h.controller.setFilter('turns');
  h.controller.setScope({ session: S1 as never, turn: null });
  expect(h.state()).toMatchObject({ filter: 'turns' });
  expect(h.calls()).toBe(1);
  // A session, or one turn of it.
  const scoped = (session: string, turn: string | null) =>
    positions(
      shownEvents(h.state().events, 'all', {
        session: session as never,
        turn: turn as never,
      }),
    );
  expect(scoped(S1, null)).toEqual(['5', '4', '3', '1']);
  expect(scoped(S1, T1)).toEqual(['4', '3']);
  expect(scoped(S1, T2)).toEqual([]);
  expect(scoped(S2, null)).toEqual([]);
  // A selection is of an event that is shown: once it is gone, so is the selection.
  h.controller.select({ stream: STREAM, sequence: '3' });
  expect(h.state().selected).toEqual({ stream: STREAM, sequence: '3' });
  h.controller.observe(connected('400'));
  h.latest[1].resolve(page(400, 50));
  await settle();
  expect(h.state().selected).toBeNull();
});

it('has nothing that changes anything, and asks nothing once disposed', async () => {
  vi.useFakeTimers();
  const h = await showing(120, 50);
  // Two reads are all a client has. The controller offers reading, choosing what to
  // look at and nothing else: nothing approves, cancels, starts, stops, resumes,
  // closes, revokes, retries an operation, performs an event again or writes a file.
  expect(Object.keys(h.client).sort()).toEqual(['latest', 'older']);
  const actions = Object.getOwnPropertyNames(EventsController.prototype);
  expect(
    actions.filter((name) =>
      /approve|deny|decide|cancel|retry|start|stop|resume|close|revoke|grant|replay|redo|undo|run|export|save|write|delete|send/i.test(
        name,
      ),
    ),
  ).toEqual([]);
  const calls = h.calls();
  const published = h.published();
  h.controller.dispose();
  h.controller.observe(connected('200'));
  h.controller.setVisible(false);
  h.controller.setVisible(true);
  h.controller.setAtTop(false);
  h.controller.setAtTop(true);
  h.controller.refresh();
  h.controller.loadOlder();
  h.controller.select({ stream: STREAM, sequence: '120' });
  h.controller.setFilter('turns');
  h.controller.setScope(null);
  h.controller.acknowledge();
  await vi.advanceTimersByTimeAsync(600_000);
  expect(h.calls()).toBe(calls);
  expect(h.published()).toBe(published);
  expect(vi.getTimerCount()).toBe(0);
});

it('says when an event was recorded to the second, and on which day when not today', () => {
  const at = '2026-10-09T10:02:07Z';
  const local = new Date(at);
  // Several events share a minute: the second is part of the time shown.
  expect(eventClock(at)).toContain(String(local.getSeconds()).padStart(2, '0'));
  expect(eventClock(at)).not.toBe(eventClock('2026-10-09T10:02:08Z'));
  // The day is said only when it is another one, and the year only when it is another.
  expect(eventDay(at, local.getTime() + 60_000)).toBe('');
  const nextDay = eventDay(at, local.getTime() + 36 * 3_600_000);
  expect(nextDay).toContain(String(local.getDate()));
  expect(nextDay).not.toContain(String(local.getFullYear()));
  expect(eventDay(at, local.getTime() + 400 * 86_400_000)).toContain(
    String(local.getFullYear()),
  );
  // What is not a time is shown as it came, and has no day.
  expect(eventClock('nonsense')).toBe('nonsense');
  expect(eventDay('nonsense')).toBe('');
});
