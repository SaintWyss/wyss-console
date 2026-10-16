import { afterEach, expect, it, vi } from 'vitest';
import {
  AgentController,
  FOLLOW_FLOOR_MS,
  FOLLOW_RETRY_MS,
  MAX_DRAFT_CHARS,
  MAX_TABS,
  MAX_TURN_CHARS,
  MAX_TURN_ITEMS,
  promptFits,
  type TranscriptItem,
} from '../src/lib/features/agent/controller';
import { runHint } from '../src/lib/features/agent/present';
import { parseManagedSessionId } from '../src/lib/contracts/identities';
import {
  A1,
  harness,
  listing,
  reading,
  S,
  S2,
  showing,
  started,
  T1,
  T2,
  T3,
  T9,
  turn,
  type Told,
} from './agent-harness';
import { connected, offline, settle } from './sessions-harness';

afterEach(() => vi.useRealTimers());

const texts = (items: TranscriptItem[]) =>
  items.flatMap((item) => (item.type === 'text' ? [item.text] : []));
const kinds = (items: TranscriptItem[]) =>
  items.map((item) => (item.type === 'activity' ? item.kind : item.type));
const UNAVAILABLE = { code: 'unavailable', retry: true } as const;

it('reads the turns of the shown session once, and nothing more while nothing changes', async () => {
  vi.useFakeTimers();
  const h = harness();
  h.controller.open(S, 'rag-corp');
  // Not connected and not shown: nothing is asked.
  expect(h.calls()).toBe(0);
  h.controller.observe(connected('9'));
  expect(h.calls()).toBe(0);
  h.controller.setVisible(true);
  expect(h.lists.map((l) => l.session)).toEqual([S]);
  expect(h.shown().reading).toBe(true);
  // Asked again while that read is in flight: it is one read, and one more after it.
  h.controller.refresh();
  h.controller.refresh();
  expect(h.lists).toHaveLength(1);
  h.lists[0].resolve(listing([]));
  await settle();
  expect(h.lists).toHaveLength(2);
  h.lists[1].resolve(listing([]));
  await settle();
  expect(h.shown()).toMatchObject({
    session: S,
    label: 'rag-corp',
    turns: [],
    runnable: true,
    reading: false,
    active: null,
  });
  expect(h.shown().facts?.binding).toBe('bound_attached');
  // No turn is running and nothing changed at the authority: a minute of nothing.
  h.controller.observe(connected('9'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(2);
  // The authority's head moved: read once for it.
  h.controller.observe(connected('10'));
  h.controller.observe(connected('10'));
  expect(h.lists).toHaveLength(3);
  // Whatever the answer, that head is not asked about again.
  h.lists[2].resolve(listing([], { head: '9' }));
  await settle();
  h.controller.observe(connected('10'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(3);
  // Hidden, a change at the authority is not read until the view is shown again.
  h.controller.setVisible(false);
  h.controller.observe(connected('11'));
  expect(h.lists).toHaveLength(3);
  h.controller.setVisible(true);
  expect(h.lists).toHaveLength(4);
  expect(h.reads).toHaveLength(0);
});

it('starts one turn for one submission and shows it from its receipt to its end', async () => {
  const h = await showing();
  h.controller.setDraft('Say hello.\nIn one line.');
  h.controller.start();
  // The revision that was shown, the identity of this action, and the input whole.
  expect(h.starts.map((s) => s.args)).toEqual([
    ['intent-1', S, 1, 'Say hello.\nIn one line.'],
  ]);
  expect(h.shown().start).toEqual({
    status: 'pending',
    prompt: 'Say hello.\nIn one line.',
    failure: undefined,
  });
  expect(h.shown().draft).toBe('');
  // Submitted again while it is pending, by a key or a click: nothing is sent.
  h.controller.setDraft('again');
  for (let i = 0; i < 5; i++) h.controller.start();
  h.controller.retryStart();
  expect(h.starts).toHaveLength(1);
  // A receipt is not a turn that runs: nothing is shown as started before it.
  expect(h.shown().turns).toEqual([]);

  h.starts[0].resolve({ id: T1 });
  await settle();
  expect(h.shown().start).toBeNull();
  expect(h.shown().turns).toMatchObject([
    { id: T1, turn: null, prompt: 'Say hello.\nIn one line.', items: [] },
  ]);
  expect(h.shown().active).toBe(T1);
  // Its record is read at once; whether another may start is read from the authority.
  expect(h.lists).toHaveLength(2);
  expect(h.reads.map(({ turn, after, wait }) => [turn, after, wait])).toEqual([
    [T1, null, false],
  ]);
  // The turn that was just accepted is running as far as anyone knows: no second one.
  h.controller.start();
  expect(h.starts).toHaveLength(1);

  h.reads[0].resolve(
    reading(turn(T1, 'running', 2), [
      [1, 'started'],
      [2, 'thinking'],
      [3, 'text', 'Hello, '],
      [4, 'text', 'world.'],
      [5, 'message_end'],
    ]),
  );
  await settle();
  const running = h.shown().turns[0];
  expect(running.turn?.state).toBe('running');
  // The answer, in order, as one block. Not the lifecycle, and not the reasoning.
  expect(running.items).toEqual([{ type: 'text', text: 'Hello, world.' }]);
  expect(running.thinking).toBe(false);
  // It goes on from where that reading ended, and now waits for something new.
  expect(h.reads.map(({ after, wait }) => [after, wait])).toEqual([
    [null, false],
    ['5', true],
  ]);
  h.lists[1].resolve(listing([turn(T1, 'running', 2)]));
  await settle();
  expect(h.shown().runnable).toBe(false);

  h.reads[1].resolve(
    reading(turn(T1, 'completed', 3), [
      [6, 'text', 'Bye.'],
      [7, 'completed'],
    ]),
  );
  await settle();
  const done = h.shown().turns[0];
  expect(done.turn).toMatchObject({ state: 'completed', reason: 'completed' });
  // A second message of the same turn is its own block.
  expect(texts(done.items)).toEqual(['Hello, world.', 'Bye.']);
  expect(h.shown().active).toBeNull();
  // Ended: it is not read again, and the session is read for what it allows now.
  expect(h.reads).toHaveLength(2);
  expect(h.lists).toHaveLength(3);
  h.lists[2].resolve(listing([turn(T1, 'completed', 3)]));
  await settle();
  expect(h.shown().runnable).toBe(true);
  expect(h.shown().draft).toBe('again');

  // The next submission is another turn, with an identity of its own.
  h.controller.start();
  expect(h.starts[1].args).toEqual(['intent-2', S, 1, 'again']);
  h.starts[1].resolve({ id: T2 });
  await settle();
  expect(h.shown().turns.map((t) => [t.id, t.prompt])).toEqual([
    [T1, 'Say hello.\nIn one line.'],
    [T2, 'again'],
  ]);
  // Never two readings at once.
  expect(h.flight.most).toBe(1);
});

it('never replaces a start whose answer was lost with a new one', async () => {
  const h = await showing();
  h.controller.setDraft('Do the thing.');
  h.controller.start();
  h.starts[0].reject(UNAVAILABLE);
  await settle();
  expect(h.shown().start).toEqual({
    status: 'failed',
    prompt: 'Do the thing.',
    failure: UNAVAILABLE,
  });
  // Whether it started is not known. The session is read; nothing is sent again.
  expect(h.lists).toHaveLength(2);
  h.lists[1].resolve(listing([]));
  await settle();
  // The composer cannot start another while that one is unsettled, whatever is typed.
  h.controller.setDraft('Do another thing.');
  h.controller.start();
  expect(h.starts).toHaveLength(1);
  expect(runHint('connected', h.shown())).toMatch(/Retry or dismiss/);
  // Retry is the same action: the same identity, the same input, the same revision.
  h.controller.retryStart();
  expect(h.starts[1].args).toEqual(h.starts[0].args);
  expect(h.starts[1].args).toEqual(['intent-1', S, 1, 'Do the thing.']);
  expect(h.shown().start?.status).toBe('pending');
  h.controller.retryStart();
  expect(h.starts).toHaveLength(2);
  h.starts[1].resolve({ id: T1 });
  await settle();
  // One turn, with the input that was sent for it; the other draft is untouched.
  expect(h.shown().turns.map((t) => [t.id, t.prompt])).toEqual([
    [T1, 'Do the thing.'],
  ]);
  expect(h.shown().draft).toBe('Do another thing.');
  expect(h.shown().start).toBeNull();
});

it('gives the input back when a refused start is dismissed, and retries only what may be', async () => {
  const h = await showing();
  h.controller.setDraft('Run it.');
  h.controller.start();
  h.starts[0].reject({ code: 'busy', retry: true });
  await settle();
  h.lists[1].resolve(listing([turn(T9, 'running', 2)]));
  await settle();
  // Dismissed: the text is the composer's again, and the session is read once more.
  h.controller.dismissStart();
  expect(h.shown().start).toBeNull();
  expect(h.shown().draft).toBe('Run it.');
  expect(h.lists).toHaveLength(3);
  h.lists[2].resolve(listing([]));
  await settle();
  h.reads.at(-1)?.resolve(reading(turn(T9, 'completed', 3), []));
  await settle();

  // A refusal that sending again cannot change is not sent again.
  h.controller.start();
  h.starts[1].reject({ code: 'changed', retry: false });
  await settle();
  h.controller.retryStart();
  expect(h.starts).toHaveLength(2);
  // A newer draft is not overwritten by the one that is given back.
  h.controller.setDraft('Something newer.');
  h.controller.dismissStart();
  expect(h.shown().draft).toBe('Something newer.');
  // Nothing is dismissed while it is pending.
  h.lists.at(-1)!.resolve(listing([]));
  await settle();
  h.controller.start();
  h.controller.dismissStart();
  expect(h.shown().start?.status).toBe('pending');
});

it('sends no start that what was read already rules out', async () => {
  expect(promptFits('x')).toBe(true);
  expect(promptFits('')).toBe(false);
  expect(promptFits(' \n\t ')).toBe(false);
  expect(promptFits('x'.repeat(8192))).toBe(true);
  expect(promptFits('x'.repeat(8193))).toBe(false);
  // The bound is the protocol's, in bytes: two-byte characters count twice.
  expect(promptFits('é'.repeat(4096))).toBe(true);
  expect(promptFits('é'.repeat(4097))).toBe(false);

  // Before the session was read.
  const unread = harness();
  unread.controller.open(S, 'rag-corp');
  unread.controller.setVisible(true);
  unread.controller.observe(connected('9'));
  unread.controller.setDraft('x');
  unread.controller.start();
  expect(unread.starts).toHaveLength(0);
  expect(runHint('connected', unread.shown())).toMatch(/Reading/);

  const cases: [Parameters<typeof showing>, RegExp][] = [
    [[[turn(T1, 'running', 2)]], /A turn is running/],
    [
      [[], { session: { binding: 'bound_detached' }, runnable: false }],
      /Resume/,
    ],
    [
      [
        [],
        { session: { binding: 'unbound', workspace: null }, runnable: false },
      ],
      /Activate/,
    ],
    [
      [[], { session: { state: 'closed', revision: 2 }, runnable: false }],
      /closed/,
    ],
    [[[], { runnable: false }], /cannot start now/],
  ];
  for (const [given, hint] of cases) {
    const h = await showing(...given);
    h.controller.setDraft('x');
    h.controller.start();
    expect(h.starts).toHaveLength(0);
    expect(runHint('connected', h.shown())).toMatch(hint);
  }
  const h = await showing();
  for (const draft of ['', '   \n', 'x'.repeat(8193)]) {
    h.controller.setDraft(draft);
    h.controller.start();
  }
  expect(h.starts).toHaveLength(0);
  expect(runHint('connected', h.shown())).toMatch(/longer than one turn/);
  // A draft is held within a bound of its own, far above what a turn carries.
  h.controller.setDraft('y'.repeat(MAX_DRAFT_CHARS + 10));
  expect(h.shown().draft).toHaveLength(MAX_DRAFT_CHARS);
  // Not connected: nothing is sent, and what was read is not taken for the present.
  h.controller.setDraft('x');
  h.controller.observe(offline);
  h.controller.start();
  expect(h.starts).toHaveLength(0);
  expect(h.shown().runnable).toBe(false);
  expect(runHint(h.state().link, h.shown())).toMatch(/unavailable/);
  expect(runHint('connected', { ...h.shown(), runnable: true })).toBeNull();
});

it('asks for a cancel at the shown revision and shows it as cancelling until the authority says', async () => {
  const h = await showing();
  h.controller.setDraft('Say hello.');
  h.controller.start();
  h.starts[0].resolve({ id: T1 });
  await settle();
  // Before its record was read there is no revision to cancel at.
  h.controller.cancel(T1);
  expect(h.cancels).toHaveLength(0);
  h.lists[1].resolve(listing([turn(T1, 'starting')]));
  h.reads[0].resolve(reading(turn(T1, 'running', 2), [[1, 'started']]));
  await settle();

  h.controller.cancel(T1);
  expect(h.cancels.map((c) => c.args)).toEqual([['intent-2', T1, 2]]);
  expect(h.shown().turns[0].cancel).toBe('requesting');
  // Asked once: a second click sends nothing.
  h.controller.cancel(T1);
  h.controller.cancel(T1);
  expect(h.cancels).toHaveLength(1);
  h.cancels[0].resolve({ id: T1 });
  await settle();
  // A receipt is not a cancelled turn. It is still what the authority last said.
  expect(h.shown().turns[0]).toMatchObject({ cancel: 'requested' });
  expect(h.shown().turns[0].turn?.state).toBe('running');
  expect(h.shown().active).toBe(T1);
  h.controller.cancel(T1);
  expect(h.cancels).toHaveLength(1);

  h.reads[1].resolve(
    reading(turn(T1, 'interrupted', 3), [[2, 'interrupted']], {}),
  );
  await settle();
  expect(h.shown().turns[0]).toMatchObject({ cancel: 'idle' });
  expect(h.shown().turns[0].turn).toMatchObject({
    state: 'interrupted',
    reason: 'user_cancel',
  });
  // Ended: there is nothing to cancel.
  h.controller.cancel(T1);
  expect(h.cancels).toHaveLength(1);
});

it('shows a cancel that was refused or lost as what it is, and the turn as it was read', async () => {
  const h = await showing();
  await started(h);
  h.reads[0].resolve(reading(turn(T1, 'running', 2), [[1, 'started']]));
  await settle();
  h.controller.cancel(T1);
  h.cancels[0].reject({ code: 'changed', retry: false });
  await settle();
  expect(h.shown().turns[0]).toMatchObject({
    cancel: 'idle',
    cancelFailure: { code: 'changed', retry: false },
  });
  expect(h.shown().turns[0].turn?.state).toBe('running');
  // The turn is read again; asking again is a new request at the revision then shown.
  h.reads[1].resolve(
    reading(turn(T1, 'running', 3), [[2, 'text', 'x']], { next: '2' }),
  );
  await settle();
  h.controller.cancel(T1);
  expect(h.cancels[1].args).toEqual(['intent-3', T1, 3]);
  expect(h.shown().turns[0].cancelFailure).toBeNull();
  // Lost: not known to be taken. Nothing says "cancelled" until a reading does.
  h.cancels[1].reject(UNAVAILABLE);
  await settle();
  expect(h.shown().turns[0]).toMatchObject({
    cancel: 'idle',
    cancelFailure: UNAVAILABLE,
  });
  expect(h.shown().turns[0].turn?.state).toBe('running');
  // It had in fact been taken: the reading says so, and the failure is not kept on a
  // turn that ended.
  h.controller.cancel(T1);
  h.reads[2].resolve(reading(turn(T1, 'interrupted', 4), [[3, 'interrupted']]));
  await settle();
  h.cancels[2].reject({ code: 'not_allowed', retry: false });
  await settle();
  expect(h.shown().turns[0]).toMatchObject({ cancel: 'idle' });
  expect(h.shown().turns[0].turn?.state).toBe('interrupted');
});

it('waits between readings that bring nothing, and goes on reading a turn while hidden', async () => {
  vi.useFakeTimers();
  const h = await showing();
  await started(h);
  h.reads[0].resolve(reading(turn(T1, 'running', 2), [[1, 'started']]));
  await settle();
  expect(h.reads).toHaveLength(2);
  // The desktop answered at once with nothing: the next reading is not immediate.
  for (let n = 2; n < 6; n++) {
    h.reads[n - 1].resolve(reading(turn(T1, 'running', 2), [], { next: '1' }));
    await settle();
    expect(h.reads).toHaveLength(n);
    await vi.advanceTimersByTimeAsync(FOLLOW_FLOOR_MS - 1);
    expect(h.reads).toHaveLength(n);
    await vi.advanceTimersByTimeAsync(1);
    expect(h.reads).toHaveLength(n + 1);
    expect(h.reads[n]).toMatchObject({ after: '1', wait: true });
  }
  // Another view is shown: the turn is not cancelled and goes on being read.
  h.controller.setVisible(false);
  h.reads[5].resolve(reading(turn(T1, 'running', 2), [[2, 'text', 'while ']]));
  await settle();
  h.reads[6].resolve(reading(turn(T1, 'running', 2), [[3, 'text', 'away']]));
  await settle();
  expect(h.cancels).toHaveLength(0);
  expect(h.starts).toHaveLength(1);
  expect(texts(h.shown().turns[0].items)).toEqual(['while away']);
  // The session's list is not read while hidden, even as the authority moves on.
  const lists = h.lists.length;
  h.controller.observe(connected('12'));
  expect(h.lists).toHaveLength(lists);
  h.controller.setVisible(true);
  expect(h.lists).toHaveLength(lists + 1);
  expect(h.flight.most).toBe(1);
});

it('takes each position once, in order, and says where output is missing', async () => {
  const h = await showing();
  await started(h);
  const running = turn(T1, 'running', 2);
  const tell = async (events: Told[], over = {}) => {
    h.reads.at(-1)!.resolve(reading(running, events, over));
    await settle();
  };
  await tell([
    [1, 'started'],
    [2, 'text', 'one '],
    [3, 'text', 'two '],
  ]);
  // The same positions again, and older ones: nothing is shown twice.
  await tell(
    [
      [2, 'text', 'one '],
      [3, 'text', 'two '],
      [4, 'text', 'three '],
    ],
    { next: '4' },
  );
  expect(texts(h.shown().turns[0].items)).toEqual(['one two three ']);
  expect(h.reads.at(-1)!.after).toBe('4');
  // A cursor that goes back is not followed.
  const asked = h.reads.length;
  await tell([], { next: '2' });
  // It brought nothing: the next reading waits a moment, and is not asked for yet.
  expect(h.reads).toHaveLength(asked);
  // A step over positions nothing accounts for is output that is missing, and said.
  h.controller.refresh();
  await settle();
  const last = h.reads.at(-1)!;
  expect(last.after).toBe('4');
  last.resolve(
    reading(running, [
      [8, 'text', 'eight '],
      [9, 'text', 'nine'],
    ]),
  );
  await settle();
  expect(h.shown().turns[0].items).toEqual([
    { type: 'text', text: 'one two three ' },
    { type: 'gap' },
    { type: 'text', text: 'eight nine' },
  ]);
  // The authority's own mark for what it dropped is one mark, however it is repeated.
  await tell(
    [
      [19, 'gap'],
      [20, 'text', 'twenty'],
    ],
    { gap: true },
  );
  await tell(
    [
      [29, 'gap'],
      [30, 'gap'],
      [31, 'text', '!'],
    ],
    { gap: true },
  );
  expect(kinds(h.shown().turns[0].items)).toEqual([
    'text',
    'gap',
    'text',
    'gap',
    'text',
    'gap',
    'text',
  ]);
});

it('shows mediated actions as what happened and reasoning as nothing but its occurrence', async () => {
  const h = await showing();
  await started(h);
  h.reads[0].resolve(
    reading(turn(T1, 'running', 2), [
      [1, 'started'],
      [2, 'thinking'],
    ]),
  );
  await settle();
  // Reasoning is a state of the turn, never an item of its transcript.
  expect(h.shown().turns[0]).toMatchObject({ thinking: true, items: [] });
  h.reads[1].resolve(
    reading(turn(T1, 'running', 2), [
      [3, 'text', 'Let me check.'],
      [4, 'message_end'],
      [5, 'command_proposed'],
      [6, 'approval_required'],
      [7, 'approval_resolved', 'accepted'],
      [8, 'command_started'],
      [9, 'command_output'],
      [10, 'command_output'],
      [11, 'command_output'],
      [12, 'command_completed'],
      [13, 'file_change_proposed'],
      [14, 'approval_resolved', null],
      [15, 'thinking'],
    ]),
  );
  await settle();
  expect(h.shown().turns[0].items).toEqual([
    { type: 'text', text: 'Let me check.' },
    { type: 'activity', kind: 'command_proposed', count: 1 },
    { type: 'activity', kind: 'approval_required', count: 1 },
    { type: 'resolved', outcome: 'accepted' },
    { type: 'activity', kind: 'command_started', count: 1 },
    // Output of a command is counted, not shown: the window is not given it.
    { type: 'activity', kind: 'command_output', count: 3 },
    { type: 'activity', kind: 'command_completed', count: 1 },
    { type: 'activity', kind: 'file_change_proposed', count: 1 },
    { type: 'resolved', outcome: null },
  ]);
  expect(h.shown().turns[0].thinking).toBe(true);
  // What the turn waits on is what the last reading says, and gone when it ends.
  const notice = {
    id: A1,
    kind: 'file_change',
    expires_at: '2026-10-09T10:15:00Z',
  } as const;
  h.reads[2].resolve(
    reading(turn(T1, 'running', 2), [[16, 'approval_required']], {
      approval: notice,
    }),
  );
  await settle();
  expect(h.shown().turns[0].approval).toEqual(notice);
  expect(h.shown().turns[0].turn?.state).toBe('running');
  h.reads[3].resolve(
    reading(turn(T1, 'failed', 3, { reason: 'approval_expired' }), [
      [17, 'approval_resolved', 'expired'],
      [18, 'failed'],
    ]),
  );
  await settle();
  expect(h.shown().turns[0]).toMatchObject({ approval: null, thinking: false });
  expect(h.shown().turns[0].turn).toMatchObject({
    state: 'failed',
    reason: 'approval_expired',
  });

  // Nothing here can answer an approval: the client has two reads and two intents, and
  // the controller has no action that names one.
  expect(Object.keys(h.client).sort()).toEqual([
    'cancel',
    'read',
    'start',
    'turns',
  ]);
  const actions = Object.getOwnPropertyNames(AgentController.prototype);
  expect(
    actions.filter((name) => /approv|accept|declin|allow|deny/i.test(name)),
  ).toEqual([]);
  expect(h.cancels).toHaveLength(0);
  expect(h.starts).toHaveLength(1);
});

it('finds a running turn again after the desktop restarted, without sending anything', async () => {
  // A window that knows nothing: the turn was started before it existed.
  const h = await showing([turn(T2, 'running', 2), turn(T1, 'completed', 3)]);
  expect(h.shown().turns.map((t) => [t.id, t.prompt])).toEqual([
    [T1, null],
    [T2, null],
  ]);
  expect(h.shown().active).toBe(T2);
  expect(h.shown().runnable).toBe(false);
  // What the authority still holds of the ended turn is read first, then the running
  // one is followed from the beginning of what is held.
  expect(h.reads.map(({ turn: t, after, wait }) => [t, after, wait])).toEqual([
    [T1, null, false],
  ]);
  h.reads[0].resolve(
    reading(turn(T1, 'completed', 3), [
      [1, 'started'],
      [2, 'text', 'Earlier answer.'],
      [3, 'completed'],
    ]),
  );
  await settle();
  expect(
    h.reads.map(({ turn: t, after, wait }) => [t, after, wait])[1],
  ).toEqual([T2, null, true]);
  h.reads[1].resolve(
    reading(turn(T2, 'running', 2), [
      [1, 'started'],
      [2, 'text', 'Still going'],
    ]),
  );
  await settle();
  expect(h.shown().turns.map((t) => texts(t.items))).toEqual([
    ['Earlier answer.'],
    ['Still going'],
  ]);
  // The ended turn is not read again. Nothing was started and nothing was cancelled.
  expect(h.reads.filter((r) => r.turn === T1)).toHaveLength(1);
  expect(h.starts).toHaveLength(0);
  expect(h.cancels).toHaveLength(0);
  // It can be cancelled from here, at the revision that was read.
  h.controller.cancel(T2);
  expect(h.cancels[0].args).toEqual(['intent-1', T2, 2]);
});

it('keeps what was read through an authority restart and reads what became of the turn', async () => {
  vi.useFakeTimers();
  const h = await showing();
  await started(h);
  h.reads[0].resolve(
    reading(turn(T1, 'running', 2), [
      [1, 'started'],
      [2, 'text', 'Half an ans'],
    ]),
  );
  await settle();
  const before = h.calls();
  // The authority goes away with a reading in flight.
  h.controller.observe(offline);
  expect(h.state().link).toBe('offline');
  // What was read is still shown, as what was read. Nothing is asked of nobody.
  expect(texts(h.shown().turns[0].items)).toEqual(['Half an ans']);
  expect(h.shown().turns[0].prompt).toBe('Say hello.');
  expect(h.shown().runnable).toBe(false);
  h.controller.cancel(T1);
  h.controller.refresh();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(before);
  // The answer to the reading that was in flight belongs to a connection that is gone.
  h.reads[1].resolve(reading(turn(T1, 'running', 2), [[3, 'text', 'LATE']]));
  await settle();
  expect(texts(h.shown().turns[0].items)).toEqual(['Half an ans']);

  // It is back, as another instance. The session and the turn are read; the input is
  // not sent again and no turn is started.
  h.controller.observe(connected('3', { restarts: 1 }));
  expect(h.lists).toHaveLength(3);
  expect(h.reads[2]).toMatchObject({ turn: T1, after: '2', wait: true });
  h.reads[2].resolve(
    reading(turn(T1, 'indeterminate', 3, { output: false }), [], {
      next: '2',
      gap: true,
    }),
  );
  h.lists[2].resolve(
    listing([turn(T1, 'indeterminate', 3, { output: false })], {
      head: '3',
      session: { binding: 'bound_detached' },
      runnable: false,
    }),
  );
  await settle();
  const after = h.shown().turns[0];
  expect(after.turn).toMatchObject({
    state: 'indeterminate',
    reason: 'authority_restarted',
  });
  // What it had said stays; what came after is said to be missing, once.
  expect(after.items).toEqual([
    { type: 'text', text: 'Half an ans' },
    { type: 'gap' },
  ]);
  expect(after.prompt).toBe('Say hello.');
  expect(h.starts).toHaveLength(1);
  // An ended turn with nothing held is never read again.
  const reads = h.reads.length;
  h.controller.observe(connected('4', { restarts: 1 }));
  h.lists
    .at(-1)!
    .resolve(
      listing([turn(T1, 'indeterminate', 3, { output: false })], { head: '4' }),
    );
  await settle();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.reads).toHaveLength(reads);
  expect(h.shown().turns[0].items).toHaveLength(2);
});

it('never takes an older record of a turn over a newer one', async () => {
  const h = await showing();
  h.controller.setDraft('Say hello.');
  h.controller.start();
  h.starts[0].resolve({ id: T1 });
  await settle();
  // The follower reads the turn to its end before the list, asked earlier, answers.
  h.reads[0].resolve(
    reading(turn(T1, 'completed', 3), [
      [1, 'text', 'done'],
      [2, 'completed'],
    ]),
  );
  await settle();
  expect(h.shown().turns[0].turn?.state).toBe('completed');
  h.lists[1].resolve(listing([turn(T1, 'running', 2)]));
  await settle();
  // What the list read was already behind: the turn stays ended, and is not followed.
  expect(h.shown().turns[0].turn).toMatchObject({
    state: 'completed',
    revision: 3,
  });
  expect(h.shown().active).toBeNull();
  expect(h.reads).toHaveLength(1);
  expect(h.cancels).toHaveLength(0);
});

it('says nothing is held for an ended turn the authority kept no output of', async () => {
  const h = await showing([turn(T1, 'completed', 3, { output: false })]);
  // Nothing to read, so nothing is read; and no gap is claimed in output never seen.
  expect(h.reads).toHaveLength(0);
  expect(h.shown().turns[0]).toMatchObject({ items: [], prompt: null });
  expect(h.shown().turns[0].turn?.output).toBe(false);
});

it('reads the output still held of ended turns newest first, each to its end and once', async () => {
  const h = await showing([
    turn(T3, 'completed', 3),
    turn(T2, 'failed', 3),
    turn(T1, 'completed', 3, { output: false }),
  ]);
  expect(h.reads.map((r) => [r.turn, r.after, r.wait])).toEqual([
    [T3, null, false],
  ]);
  // More is held than one reading carries: it goes on from its cursor.
  h.reads[0].resolve(
    reading(turn(T3, 'completed', 3), [[1, 'text', 'a']], { more: true }),
  );
  await settle();
  expect(h.reads[1]).toMatchObject({ turn: T3, after: '1', wait: false });
  // "More" with nothing in it is not followed for ever.
  h.reads[1].resolve(
    reading(turn(T3, 'completed', 3), [], { next: '1', more: true }),
  );
  await settle();
  expect(h.reads[2]).toMatchObject({ turn: T2, after: null, wait: false });
  h.reads[2].resolve(
    reading(turn(T2, 'failed', 3), [
      [1, 'text', 'b'],
      [2, 'failed'],
    ]),
  );
  await settle();
  expect(h.reads).toHaveLength(3);
  expect(h.shown().turns.map((t) => texts(t.items))).toEqual([
    [],
    ['b'],
    ['a'],
  ]);
  // Read again by a later list, they are what they were: nothing is fetched twice.
  h.controller.observe(connected('10'));
  h.lists[1].resolve(
    listing(
      [
        turn(T3, 'completed', 3),
        turn(T2, 'failed', 3),
        turn(T1, 'completed', 3, { output: false }),
      ],
      { head: '10' },
    ),
  );
  await settle();
  expect(h.reads).toHaveLength(3);
  expect(h.flight.most).toBe(1);
});

it('stops reading a turn that could not be read, and tries again without looping', async () => {
  vi.useFakeTimers();
  const h = await showing();
  await started(h);
  h.reads[0].reject(UNAVAILABLE);
  await settle();
  expect(h.shown().turns[0].readFailure).toEqual(UNAVAILABLE);
  // Not at once, and not twice: once, after a while.
  expect(h.reads).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(FOLLOW_RETRY_MS - 1);
  expect(h.reads).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(h.reads).toHaveLength(2);
  expect(h.shown().turns[0].readFailure).toBeNull();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.reads).toHaveLength(2);

  // A failure that asking again cannot change is not asked again by itself.
  h.reads[1].reject({ code: 'not_found', retry: false });
  await settle();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.reads).toHaveLength(2);
  expect(h.shown().turns[0].readFailure?.code).toBe('not_found');
  // The user asks, or the authority changes: it is read again, once.
  h.controller.refresh();
  expect(h.reads).toHaveLength(3);
  h.reads[2].reject({ code: 'not_found', retry: false });
  await settle();
  h.controller.observe(connected('10'));
  expect(h.reads).toHaveLength(4);
  expect(h.flight.most).toBe(1);
});

it('keeps the turns, the drafts and the output of each session apart', async () => {
  const h = await showing();
  await started(h, T1, 'For the first session.');
  h.reads[0].resolve(reading(turn(T1, 'running', 2), [[1, 'text', 'FIRST']]));
  await settle();
  h.controller.setDraft('draft of the first');

  // Another session is opened and shown. The first one's turn goes on.
  expect(h.controller.open(S2, 'other')).toBe(true);
  expect(h.state().tabs).toEqual([
    { session: S, label: 'rag-corp' },
    { session: S2, label: 'other' },
  ]);
  expect(h.shown()).toMatchObject({ session: S2, turns: [], draft: '' });
  expect(h.lists.at(-1)!.session).toBe(S2);
  h.lists.at(-1)!.resolve(
    listing([{ ...turn(T2, 'completed', 3), session: S2 }], {
      session: { id: S2, workspace: '/home/dev/other' },
      runnable: false,
    }),
  );
  await settle();
  // Still one reading at a time: the first session's turn is the one being read.
  expect(h.flight.most).toBe(1);
  h.reads[1].resolve(reading(turn(T1, 'running', 2), [[2, 'text', ' MORE']]));
  await settle();
  // Its output landed in its own session and nowhere else.
  expect(JSON.stringify(h.shown())).not.toMatch(/FIRST|MORE|first/);
  expect(h.shown().turns.map((t) => t.id)).toEqual([T2]);
  // What is held of the shown session's ended turn is read, then the follower goes
  // back to the turn that runs, in the session it belongs to.
  expect(h.reads[2]).toMatchObject({ turn: T2, after: null, wait: false });
  h.reads[2].resolve(
    reading({ ...turn(T2, 'completed', 3), session: S2 }, [
      [1, 'text', 'SECOND'],
      [2, 'completed'],
    ]),
  );
  await settle();
  expect(h.reads[3]).toMatchObject({ turn: T1, after: '2', wait: true });
  expect(texts(h.shown().turns[0].items)).toEqual(['SECOND']);
  h.controller.setDraft('draft of the other');
  // A start here goes to this session, at this session's revision.
  expect(h.shown().active).toBeNull();

  h.controller.select(S);
  expect(h.shown().session).toBe(S);
  expect(h.shown().draft).toBe('draft of the first');
  expect(texts(h.shown().turns[0].items)).toEqual(['FIRST MORE']);
  expect(JSON.stringify(h.shown())).not.toMatch(/other|SECOND/);

  // A reading that says the turn belongs to another session is not shown under this one.
  const pending = h.reads.filter((r) => r.turn === T1).at(-1)!;
  pending.resolve(
    reading({ ...turn(T1, 'running', 3), session: S2 }, [
      [3, 'text', 'LEAKED'],
    ]),
  );
  await settle();
  expect(JSON.stringify(h.state())).not.toContain('LEAKED');
  expect(h.shown().turns[0].readFailure).toEqual({
    code: 'internal',
    retry: false,
  });
  expect(texts(h.shown().turns[0].items)).toEqual(['FIRST MORE']);

  // Closing a tab asks nothing of the authority, and its answers in flight are dropped.
  const calls = h.calls();
  h.controller.close(S);
  expect(h.state().tabs).toEqual([{ session: S2, label: 'other' }]);
  expect(h.shown().session).toBe(S2);
  expect(h.shown().draft).toBe('draft of the other');
  expect(h.cancels).toHaveLength(0);
  expect(h.calls() - calls).toBeLessThanOrEqual(1);
  // Opened again, it is read from the authority: the window kept nothing of it.
  h.controller.open(S, 'rag-corp');
  expect(h.shown()).toMatchObject({ session: S, turns: [], draft: '' });
});

it('ignores answers for a session that is no longer the one they were asked for', async () => {
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected('9'));
  h.controller.open(S, 'rag-corp');
  h.controller.setDraft('x');
  h.controller.close(S);
  expect(h.state().current).toBeNull();
  // Its list arrives after the tab is gone, and after it was opened again.
  h.controller.open(S, 'rag-corp');
  h.lists[0].resolve(listing([turn(T1, 'completed', 3, { output: false })]));
  await settle();
  expect(h.shown().turns).toEqual([]);
  h.lists[1].resolve(listing([]));
  await settle();
  // A start whose tab cannot be closed while it is pending.
  h.controller.setDraft('x');
  h.controller.start();
  h.controller.close(S);
  expect(h.state().tabs).toHaveLength(1);
  h.starts[0].resolve({ id: T1 });
  await settle();
  expect(h.shown().turns.map((t) => t.id)).toEqual([T1]);
});

it('holds a bounded amount of everything it shows', async () => {
  const h = await showing();
  await started(h);
  const running = turn(T1, 'running', 2);
  let at = 0;
  // Twelve megabytes of answer: sixty readings of fifty pieces of four kilobytes.
  for (let page = 0; page < 60; page++) {
    const events: Told[] = [];
    for (let i = 0; i < 50; i++) events.push([++at, 'text', 'x'.repeat(4096)]);
    h.reads.at(-1)!.resolve(reading(running, events));
    await settle();
  }
  const entry = () => h.shown().turns[0];
  const held = () => texts(entry().items).join('').length;
  expect(held()).toBe(MAX_TURN_CHARS);
  expect(entry().clipped).toBe(true);
  // What is kept is the end of the answer, not its beginning.
  h.reads.at(-1)!.resolve(reading(running, [[++at, 'text', 'THE END']]));
  await settle();
  expect(texts(entry().items).join('').endsWith('THE END')).toBe(true);
  expect(held()).toBe(MAX_TURN_CHARS);
  // Many small things: activity that never repeats, so that nothing coalesces.
  for (let page = 0; page < 20; page++) {
    const events: Told[] = [];
    for (let i = 0; i < 50; i++)
      events.push([++at, i % 2 ? 'command_started' : 'command_completed']);
    h.reads.at(-1)!.resolve(reading(running, events));
    await settle();
  }
  expect(entry().items).toHaveLength(MAX_TURN_ITEMS);
  expect(held()).toBeLessThanOrEqual(MAX_TURN_CHARS);

  // Turns: what the list names, and no more than it may name.
  const many = await showing();
  for (let n = 0; n < 30; n++) {
    many.controller.setDraft(`turn ${n}`);
    many.controller.start();
    many.starts.at(-1)!.resolve({
      id: parseTurn(n),
    });
    await settle();
    many.reads
      .at(-1)!
      .resolve(reading(turn(parseTurn(n), 'completed', 3), [[1, 'completed']]));
    await settle();
    for (const list of many.lists) list.resolve(listing([]));
    await settle();
  }
  expect(many.shown().turns.length).toBeLessThanOrEqual(20);
  // A full list, and a turn started here that the list does not name yet: still twenty,
  // before the list is read and after it.
  const listed = Array.from({ length: 20 }, (_, n) =>
    turn(parseTurn(100 + n), 'completed', 3, { output: false }),
  );
  const full = await showing(listed);
  expect(full.shown().turns).toHaveLength(20);
  full.controller.setDraft('one more');
  full.controller.start();
  full.starts[0].resolve({ id: T9 });
  await settle();
  expect(full.shown().turns).toHaveLength(20);
  expect(full.shown().turns.at(-1)!.id).toBe(T9);
  full.lists.at(-1)!.resolve(listing(listed));
  await settle();
  expect(full.shown().turns).toHaveLength(20);
  // The one that runs is the one that is kept.
  expect(full.shown().turns.at(-1)!.id).toBe(T9);

  // Tabs: a bounded number. The oldest idle one gives way; one with a turn does not.
  const tabs = await showing();
  await started(tabs);
  const session = (n: number) =>
    parseManagedSessionId(`0000000${n}-0000-4000-8000-000000000000`);
  for (let n = 1; n < MAX_TABS; n++)
    expect(tabs.controller.open(session(n), `s${n}`)).toBe(true);
  expect(tabs.state().tabs).toHaveLength(MAX_TABS);
  expect(tabs.controller.open(session(9), 's9')).toBe(true);
  expect(tabs.state().tabs).toHaveLength(MAX_TABS);
  // The session with a running turn is still there; an idle one made room.
  expect(tabs.state().tabs.map((t) => t.label)).toContain('rag-corp');
  expect(tabs.state().tabs.map((t) => t.label)).not.toContain('s1');
});
function parseTurn(n: number) {
  const hex = n.toString(16).padStart(2, '0');
  return turn(T1, 'starting').id.replace(/^aa/, hex) as typeof T1;
}

it('asks nothing and shows nothing after it is disposed', async () => {
  vi.useFakeTimers();
  const h = await showing();
  await started(h);
  h.reads[0].resolve(reading(turn(T1, 'running', 2), [], { next: null }));
  await settle();
  const calls = h.calls();
  const published = h.published();
  // The reading brought nothing, so the next one is waiting on a timer.
  expect(vi.getTimerCount()).toBe(1);
  h.controller.dispose();
  // Nothing is left to fire.
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(60_000);
  h.controller.observe(connected('20'));
  h.controller.setVisible(false);
  h.controller.setVisible(true);
  h.controller.open(S2, 'x');
  h.controller.setDraft('x');
  h.controller.start();
  h.controller.cancel(T1);
  h.controller.refresh();
  h.controller.close(S);
  for (const read of h.reads) read.resolve(reading(turn(T1, 'running', 2), []));
  await settle();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(calls);
  expect(h.published()).toBe(published);
});

/** A session opened under `label`, shown, and its first list answered. */
async function showingAs(label: string, provisional: boolean) {
  const h = harness();
  h.controller.open(S, label, provisional);
  h.controller.setVisible(true);
  h.controller.observe(connected('9'));
  h.lists[0].resolve(listing([]));
  await settle();
  return h;
}

it('names a tab from the folder the authority reports when it was opened without a name', async () => {
  const h = await showingAs('Session 22222222', true);
  expect(h.state().tabs).toEqual([{ session: S, label: 'rag-corp' }]);
  expect(h.shown().label).toBe('rag-corp');
});

it('keeps the name it was given, and uses a better one when the session is opened again', async () => {
  const h = await showingAs('rag-corp · Oct 8, 2026, 11:00 AM', false);
  expect(h.state().tabs[0].label).toBe('rag-corp · Oct 8, 2026, 11:00 AM');
  // A stand-in never replaces a name that was given.
  h.controller.open(S, 'Session 22222222', true);
  expect(h.state().tabs[0].label).toBe('rag-corp · Oct 8, 2026, 11:00 AM');
  // A name from where it is known replaces the one held.
  h.controller.open(S, 'rag-corp', false);
  expect(h.state().tabs[0].label).toBe('rag-corp');
  expect(h.state().tabs).toHaveLength(1);
});

it('never shows two tabs under one name', async () => {
  const h = harness();
  h.controller.observe(connected('9'));
  h.controller.open(S, 'rag-corp');
  expect(h.state().tabs.map((t) => t.label)).toEqual(['rag-corp']);
  h.controller.open(S2, 'rag-corp');
  const labels = h.state().tabs.map((t) => t.label);
  expect(labels).toEqual([
    `rag-corp · ${S.slice(0, 8)}`,
    `rag-corp · ${S2.slice(0, 8)}`,
  ]);
  expect(new Set(labels).size).toBe(2);
  // The session shown carries the same name as its tab.
  expect(h.shown().label).toBe(`rag-corp · ${S2.slice(0, 8)}`);
  // Once they differ, nothing is added.
  h.controller.open(S2, 'other');
  expect(h.state().tabs.map((t) => t.label)).toEqual(['rag-corp', 'other']);
});

it('keeps tab names unique when a name equals the short identity another tab gained', () => {
  const h = harness();
  h.controller.observe(connected('9'));
  // Two tabs under one name gain their short identities; a third is called exactly that.
  h.controller.open(S, 'rag-corp');
  h.controller.open(S2, 'rag-corp');
  h.controller.open(
    '99999999-9999-4999-8999-999999999999' as typeof S,
    `rag-corp · ${S.slice(0, 8)}`,
  );
  const labels = h.state().tabs.map((t) => t.label);
  expect(new Set(labels).size).toBe(3);
  expect(h.shown().label).toBe(labels[2]);
});
