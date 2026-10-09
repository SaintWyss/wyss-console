import { afterEach, expect, it, vi } from 'vitest';
import {
  parseApprovalDetail,
  parseApprovalSummary,
  type ApprovalDetail,
  type ApprovalList,
  type ApprovalSummary,
  type DecisionReceipt,
} from '../src/lib/contracts/approvals';
import {
  parseManagedApprovalId,
  parseManagedSessionId,
  type ManagedApprovalId,
} from '../src/lib/contracts/identities';
import type { SessionFailure } from '../src/lib/contracts/sessions';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import {
  ApprovalsController,
  EXPIRY_GRACE_MS,
  FILES_AT_ONCE,
  type ApprovalsClient,
  type ApprovalsState,
} from '../src/lib/features/approvals/controller';
import { wholeShown } from '../src/lib/features/approvals/present';
import fixture from './fixtures/approval-list-v1.json';
import { connected, offline, settle, type Deferred } from './sessions-harness';

afterEach(() => vi.useRealTimers());

const C = parseManagedApprovalId(fixture.command.approval.id);
const F = parseManagedApprovalId(fixture.file.approval.id);
const UNAVAILABLE = { code: 'unavailable', retry: true } as const;
/** The command approval, with its record changed by `over`. */
const command = (over: Partial<ApprovalSummary> = {}): ApprovalDetail =>
  parseApprovalDetail({
    ...fixture.command,
    approval: { ...fixture.command.approval, ...over },
  });
/** A change set of `files` files, showing file `index` from `offset`. */
function change(
  files: number,
  index = 0,
  page: {
    offset?: number;
    chunk?: string;
    bytes?: number;
    next?: number | null;
  } = {},
): ApprovalDetail {
  const chunk = page.chunk ?? `+file ${index}\n`;
  const offset = page.offset ?? 0;
  return parseApprovalDetail({
    ...fixture.file,
    files,
    file: {
      index,
      path: `/home/dev/rag-corp/src/file-${index}.rs`,
      kind: 'update',
      move_path: null,
      bytes: page.bytes ?? offset + chunk.length,
      offset,
      chunk,
      next: page.next ?? null,
    },
  });
}
const ended = fixture.recent.map((r) => parseApprovalSummary(r));
const listOf = (
  pending: ApprovalDetail[],
  recent: ApprovalSummary[] = [],
  head = '9',
): ApprovalList => ({
  schema_version: 1,
  head,
  pending,
  recent,
  partial: false,
});

function harness() {
  const lists: Deferred<ApprovalList>[] = [];
  const reads: (Deferred<ApprovalDetail> & {
    approval: ManagedApprovalId;
    file: number | null;
    offset: number | null;
  })[] = [];
  const decisions: (Deferred<DecisionReceipt> & {
    kind: 'approve' | 'deny';
    args: unknown[];
  })[] = [];
  const flight = { now: 0, most: 0 };
  const deferred = <T>(): Deferred<T> & { promise: Promise<T> } => {
    let resolve!: (value: T) => void;
    let fail!: (error: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      fail = rej;
    });
    return {
      promise,
      resolve,
      reject: (failure: SessionFailure) => fail(new SessionCallError(failure)),
    };
  };
  const decide =
    (kind: 'approve' | 'deny') =>
    (...args: unknown[]) => {
      const d = deferred<DecisionReceipt>();
      decisions.push({ ...d, kind, args });
      return d.promise;
    };
  const client: ApprovalsClient = {
    list() {
      const d = deferred<ApprovalList>();
      lists.push(d);
      return d.promise;
    },
    read(approval, file, offset) {
      const d = deferred<ApprovalDetail>();
      flight.now++;
      flight.most = Math.max(flight.most, flight.now);
      const landed = () => {
        flight.now--;
      };
      void d.promise.then(landed, landed);
      reads.push({ ...d, approval, file, offset });
      return d.promise;
    },
    approve: decide('approve'),
    deny: decide('deny'),
  };
  let state!: ApprovalsState;
  let minted = 0;
  let published = 0;
  const controller = new ApprovalsController(
    (next) => {
      state = next;
      published++;
    },
    client,
    () => `intent-${++minted}`,
  );
  state = controller.state();
  return {
    controller,
    client,
    lists,
    reads,
    decisions,
    flight,
    state: () => state,
    card: (id: ManagedApprovalId = C) =>
      state.pending!.find((c) => c.approval.id === id)!,
    published: () => published,
    calls: () => lists.length + reads.length + decisions.length,
  };
}
/** The Approvals view shown and connected, its first list answered with `list`. */
async function showing(list: ApprovalList = listOf([command()])) {
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected('9'));
  h.lists[0].resolve(list);
  await settle();
  return h;
}

it('reads approvals only while somebody has a use for them, once per change', async () => {
  vi.useFakeTimers();
  const h = harness();
  // Connected, with no view shown and no turn followed: nothing is read. Inference
  // work adds nothing here.
  h.controller.observe(connected('9'));
  h.controller.observe(connected('10'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(0);
  expect(h.state().pending).toBeNull();

  // A turn is being followed: an approval may be asked for at any moment.
  h.controller.setWatching(true);
  expect(h.lists).toHaveLength(1);
  expect(h.state().reading).toBe(true);
  h.lists[0].resolve(listOf([], [], '10'));
  await settle();
  expect(h.state()).toMatchObject({ pending: [], recent: [], reading: false });
  // Nothing changed at the authority: a minute of nothing, with no timer running.
  h.controller.observe(connected('10'));
  await vi.advanceTimersByTimeAsync(60_000);
  expect(h.calls()).toBe(1);
  expect(vi.getTimerCount()).toBe(0);
  // The durable head moved: read once for it, whatever the answer.
  h.controller.observe(connected('11'));
  h.controller.observe(connected('11'));
  expect(h.lists).toHaveLength(2);
  h.lists[1].resolve(listOf([], [], '10'));
  await settle();
  h.controller.observe(connected('11'));
  expect(h.lists).toHaveLength(2);
  // Nobody is interested any more: a change is not read until somebody is.
  h.controller.setWatching(false);
  h.controller.observe(connected('12'));
  expect(h.lists).toHaveLength(2);
  h.controller.setVisible(true);
  expect(h.lists).toHaveLength(3);
  // Asked again while that read is in flight: one read, and one more after it.
  h.controller.refresh();
  h.controller.refresh();
  expect(h.lists).toHaveLength(3);
  h.lists[2].resolve(listOf([command()], ended, '12'));
  await settle();
  expect(h.lists).toHaveLength(4);
});

it('shows what the authority holds and nothing read on a connection that is gone', async () => {
  const h = await showing(listOf([command()], ended));
  expect(h.state().pending).toHaveLength(1);
  expect(h.card()).toMatchObject({
    approval: { id: C, state: 'pending', decidable: true },
    workspace: '/home/dev/rag-corp',
    command: fixture.command.command,
    files: null,
    shown: [],
    decision: null,
  });
  expect(h.state().recent.map((r) => r.state)).toEqual(['accepted', 'expired']);
  // A command is whole as it comes: there is nothing more to read for it.
  expect(wholeShown(h.card())).toBe(true);
  expect(h.reads).toHaveLength(0);

  // The authority goes away. Nothing that was pending is shown as pending: what there
  // is to decide is read again from whoever answers next.
  h.controller.observe(offline);
  expect(h.state()).toMatchObject({
    link: 'offline',
    pending: null,
    recent: [],
  });
  h.controller.decide(C, 'approve');
  h.controller.refresh();
  expect(h.decisions).toHaveLength(0);
  // The answer to a list asked on the connection that is gone is not shown either.
  const before = h.lists.length;
  h.controller.observe(connected('3', { restarts: 1 }));
  expect(h.lists).toHaveLength(before + 1);
  // It restarted: the approval it held was invalidated, and that is what it says.
  h.lists.at(-1)!.resolve(
    listOf(
      [],
      [
        parseApprovalSummary({
          ...fixture.recent[1],
          id: C,
          state: 'invalidated',
          reason: 'authority_restarted',
        }),
      ],
      '3',
    ),
  );
  await settle();
  expect(h.state().pending).toEqual([]);
  expect(h.state().recent[0]).toMatchObject({
    id: C,
    state: 'invalidated',
    reason: 'authority_restarted',
  });
  // Nothing was decided for anybody on the way back.
  expect(h.decisions).toHaveLength(0);
});

it('sends one decision for one approval, at the revision that is shown', async () => {
  const h = await showing(listOf([command({ revision: 3 })]));
  h.controller.decide(C, 'approve');
  expect(h.decisions.map((d) => [d.kind, d.args])).toEqual([
    ['approve', ['intent-1', C, 3]],
  ]);
  expect(h.card().decision).toEqual({
    kind: 'approve',
    status: 'pending',
    reread: false,
  });
  // Clicked again, or the other button: nothing more is sent while it is pending.
  for (let i = 0; i < 4; i++) h.controller.decide(C, 'approve');
  h.controller.decide(C, 'deny');
  h.controller.retry(C);
  h.controller.dismiss(C);
  expect(h.decisions).toHaveLength(1);

  // The authority committed it. What the approval is now is read, and until a list
  // says so it is shown as being decided, not as decided.
  h.decisions[0].resolve({ id: C, state: 'accepted' });
  await settle();
  expect(h.lists).toHaveLength(2);
  expect(h.card().decision?.status).toBe('pending');
  expect(h.card().approval.state).toBe('pending');
  h.lists[1].resolve(
    listOf(
      [],
      [
        parseApprovalSummary({
          ...fixture.recent[0],
          id: C,
          revision: 4,
        }),
      ],
    ),
  );
  await settle();
  expect(h.state().pending).toEqual([]);
  expect(h.state().recent[0]).toMatchObject({ id: C, state: 'accepted' });
  // Gone from what can be decided: nothing is sent for it again.
  h.controller.decide(C, 'approve');
  h.controller.decide(C, 'deny');
  expect(h.decisions).toHaveLength(1);

  // Deny is its own request, and never anything else.
  const d = await showing();
  d.controller.decide(C, 'deny');
  expect(d.decisions.map((x) => [x.kind, x.args])).toEqual([
    ['deny', ['intent-1', C, 1]],
  ]);
  // An approval the authority does not offer for a decision is not decided.
  const closed = await showing(listOf([command({ decidable: false })]));
  closed.controller.decide(C, 'approve');
  closed.controller.decide(C, 'deny');
  expect(closed.decisions).toHaveLength(0);
});

it('never replaces a decision whose answer was lost, nor turns it into the other one', async () => {
  const h = await showing();
  h.controller.decide(C, 'approve');
  h.decisions[0].reject(UNAVAILABLE);
  await settle();
  // Not known to be taken. The approval is read again; nothing is sent by itself.
  expect(h.card().decision).toEqual({
    kind: 'approve',
    status: 'failed',
    failure: UNAVAILABLE,
    reread: false,
  });
  expect(h.lists).toHaveLength(2);
  expect(h.decisions).toHaveLength(1);
  // Until that reading is back the decision cannot be let go of, and no other is sent.
  h.controller.dismiss(C);
  h.controller.decide(C, 'deny');
  h.controller.decide(C, 'approve');
  expect(h.card().decision?.status).toBe('failed');
  expect(h.decisions).toHaveLength(1);
  // Retry is the same action: the same identity, the same decision, the same revision.
  h.controller.retry(C);
  expect(h.decisions[1].kind).toBe('approve');
  expect(h.decisions[1].args).toEqual(h.decisions[0].args);
  expect(h.decisions[1].args).toEqual(['intent-1', C, 1]);
  h.controller.retry(C);
  expect(h.decisions).toHaveLength(2);
  h.decisions[1].reject(UNAVAILABLE);
  h.lists[1].resolve(listOf([command()]));
  await settle();
  h.lists.at(-1)!.resolve(listOf([command()]));
  await settle();
  // Read again, and still there to decide: the request was never taken. Only now can
  // the user let go of it, and what is asked next is another action of theirs.
  expect(h.card().decision).toMatchObject({ status: 'failed', reread: true });
  h.controller.decide(C, 'deny');
  expect(h.decisions).toHaveLength(2);
  h.controller.dismiss(C);
  expect(h.card().decision).toBeNull();
  h.controller.decide(C, 'deny');
  expect(h.decisions[2].kind).toBe('deny');
  expect(h.decisions[2].args).toEqual(['intent-2', C, 1]);

  // The other way it can end: the lost decision had been taken. The list says so, the
  // card is gone, and nothing is left to retry.
  const taken = await showing();
  taken.controller.decide(C, 'approve');
  taken.decisions[0].reject(UNAVAILABLE);
  await settle();
  taken.lists[1].resolve(
    listOf([], [parseApprovalSummary({ ...fixture.recent[0], id: C })]),
  );
  await settle();
  expect(taken.state().pending).toEqual([]);
  taken.controller.retry(C);
  taken.controller.decide(C, 'deny');
  expect(taken.decisions).toHaveLength(1);
});

it('shows a refusal as a refusal and reads what the approval became', async () => {
  for (const failure of [
    { code: 'changed', retry: false },
    { code: 'not_allowed', retry: false },
    { code: 'permission_denied', retry: false },
  ] as const) {
    const h = await showing();
    h.controller.decide(C, 'approve');
    h.decisions[0].reject(failure);
    await settle();
    expect(h.card().decision).toMatchObject({ status: 'failed', failure });
    // A refusal that sending again cannot change is not sent again.
    h.controller.retry(C);
    expect(h.decisions).toHaveLength(1);
    expect(h.lists).toHaveLength(2);
    // It had ended in the meantime: that is what is shown.
    h.lists[1].resolve(listOf([], [ended[1]]));
    await settle();
    expect(h.state().pending).toEqual([]);
  }
});

it('reads what became of an approval at its deadline, once, and keeps no timer after', async () => {
  vi.useFakeTimers();
  const start = Date.parse('2026-10-09T10:14:00Z');
  vi.setSystemTime(start);
  const h = await showing(
    listOf([command({ expires_at: '2026-10-09T10:15:00Z' })]),
  );
  // One timer, for the one deadline. Nothing is read before it.
  expect(vi.getTimerCount()).toBe(1);
  await vi.advanceTimersByTimeAsync(60_000 + EXPIRY_GRACE_MS - 1);
  expect(h.lists).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(h.lists).toHaveLength(2);
  // The authority ended it: it is shown ended, and there is nothing to decide.
  h.lists[1].resolve(
    listOf(
      [],
      [
        parseApprovalSummary({
          ...fixture.recent[1],
          id: C,
          kind: 'command_execution',
        }),
      ],
    ),
  );
  await settle();
  expect(h.state().pending).toEqual([]);
  expect(h.state().recent[0]).toMatchObject({ id: C, state: 'expired' });
  h.controller.decide(C, 'approve');
  expect(h.decisions).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(600_000);
  expect(h.lists).toHaveLength(2);

  // A decision that races the deadline: the authority wins, and its refusal is shown.
  vi.setSystemTime(start);
  const race = await showing(
    listOf([command({ expires_at: '2026-10-09T10:15:00Z' })]),
  );
  race.controller.decide(C, 'approve');
  race.decisions[0].reject({ code: 'not_allowed', retry: false });
  await settle();
  expect(race.card().decision?.failure?.code).toBe('not_allowed');
  // Disposed with a deadline pending: nothing is left to fire.
  expect(vi.getTimerCount()).toBe(1);
  race.controller.dispose();
  expect(vi.getTimerCount()).toBe(0);
});

it('reads a change set file by file and a diff to its end, one request at a time', async () => {
  const h = await showing(listOf([change(11)]));
  // The first file came with the list. The others are read by themselves, in order,
  // one at a time, and only so many.
  expect(h.card(F).shown.map((f) => f.index)).toEqual([0]);
  expect(wholeShown(h.card(F))).toBe(false);
  for (let index = 1; index <= FILES_AT_ONCE; index++) {
    expect(h.reads).toHaveLength(index);
    expect(h.reads.at(-1)).toMatchObject({
      approval: F,
      file: index,
      offset: 0,
    });
    h.reads.at(-1)!.resolve(change(11, index));
    await settle();
  }
  expect(h.reads).toHaveLength(FILES_AT_ONCE);
  expect(h.card(F).shown).toHaveLength(FILES_AT_ONCE + 1);
  expect(h.card(F).listing).toBe(false);
  // Not all of it is shown, and that is known: nine of eleven.
  expect(wholeShown(h.card(F))).toBe(false);
  h.controller.moreFiles(F);
  for (const index of [9, 10]) {
    expect(h.reads.at(-1)).toMatchObject({ file: index, offset: 0 });
    h.reads.at(-1)!.resolve(change(11, index));
    await settle();
  }
  expect(h.card(F).shown.map((f) => f.index)).toEqual([
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
  ]);
  expect(wholeShown(h.card(F))).toBe(true);
  const settled = h.reads.length;
  h.controller.moreFiles(F);
  expect(h.reads).toHaveLength(settled);
  expect(h.flight.most).toBe(1);

  // A diff longer than a page: said to be truncated until it is read to its end.
  const long = await showing(
    listOf([change(1, 0, { chunk: 'a'.repeat(10), bytes: 25, next: 10 })]),
  );
  expect(long.card(F).shown[0]).toMatchObject({
    text: 'a'.repeat(10),
    next: 10,
  });
  expect(wholeShown(long.card(F))).toBe(false);
  long.controller.more(F, 0);
  long.controller.more(F, 0);
  expect(long.reads).toHaveLength(1);
  expect(long.reads[0]).toMatchObject({ approval: F, file: 0, offset: 10 });
  long.reads[0].resolve(
    change(1, 0, { offset: 10, chunk: 'b'.repeat(10), bytes: 25, next: 20 }),
  );
  await settle();
  // It goes on by itself to the end: the next page from where that one ended.
  expect(long.reads[1]).toMatchObject({ offset: 20 });
  long.reads[1].resolve(
    change(1, 0, { offset: 20, chunk: 'c'.repeat(5), bytes: 25 }),
  );
  await settle();
  expect(long.card(F).shown[0]).toMatchObject({
    text: `${'a'.repeat(10)}${'b'.repeat(10)}${'c'.repeat(5)}`,
    next: null,
    reading: false,
  });
  expect(wholeShown(long.card(F))).toBe(true);
  expect(long.reads).toHaveLength(2);
});

it('shows nothing in place of a page or a file that is not the one it asked for', async () => {
  const h = await showing(
    listOf([change(2, 0, { chunk: 'a'.repeat(10), bytes: 25, next: 10 })]),
  );
  // The second file is asked for; the answer is about another change set.
  h.reads[0].resolve(change(3, 1));
  await settle();
  expect(h.card(F).shown.map((f) => f.index)).toEqual([0]);
  expect(wholeShown(h.card(F))).toBe(false);
  // A page of another diff is not appended to this one.
  h.controller.more(F, 0);
  h.reads
    .at(-1)!
    .resolve(change(2, 0, { offset: 10, chunk: 'ZZZZZ', bytes: 99, next: 15 }));
  await settle();
  expect(h.card(F).shown[0]).toMatchObject({
    text: 'a'.repeat(10),
    next: 10,
    failure: { code: 'internal', retry: false },
  });
  // A page that could not be read: said, and what the approval is now is read.
  const lists = h.lists.length;
  h.controller.more(F, 0);
  h.reads.at(-1)!.reject(UNAVAILABLE);
  await settle();
  expect(h.card(F).shown[0].failure).toEqual(UNAVAILABLE);
  expect(h.lists).toHaveLength(lists + 1);
  // What was read of a change set does not carry over to another revision of it.
  h.lists.at(-1)!.resolve(
    listOf([
      parseApprovalDetail({
        ...change(2),
        approval: { ...change(2).approval, revision: 2 },
      }),
    ]),
  );
  await settle();
  expect(h.card(F).shown[0]).toMatchObject({ failure: null, next: null });
  expect(h.card(F).approval.revision).toBe(2);
});

it('reads an approval a turn named, once, and keeps approvals of different sessions apart', async () => {
  const h = harness();
  h.controller.observe(connected('9'));
  // A reading of a turn names an approval nobody has read: it is read now.
  h.controller.want(C);
  expect(h.lists).toHaveLength(1);
  h.controller.want(C);
  h.lists[0].resolve(listOf([]));
  await settle();
  // The list does not have it. It is not asked about in a loop.
  h.controller.want(C);
  h.controller.want(C);
  expect(h.lists).toHaveLength(1);
  // Something changed at the authority: it is asked about again, once.
  h.controller.observe(connected('10'));
  h.controller.want(C);
  expect(h.lists).toHaveLength(2);
  const other = parseManagedSessionId('44444444-4444-4444-8444-444444444444');
  const theirs = parseApprovalDetail({
    ...fixture.file,
    workspace: '/home/dev/other',
    approval: { ...fixture.file.approval, session: other },
  });
  h.lists[1].resolve(listOf([command(), theirs], [], '10'));
  await settle();
  // Shown, so not asked for again.
  h.controller.want(C);
  expect(h.lists).toHaveLength(2);
  // Two sessions, two cards, each with its own action and its own decision.
  expect(
    h.state().pending!.map((c) => [c.approval.id, c.approval.session]),
  ).toEqual([
    [C, fixture.command.approval.session],
    [F, other],
  ]);
  h.controller.decide(F, 'deny');
  expect(h.decisions.map((d) => [d.kind, d.args[1]])).toEqual([['deny', F]]);
  expect(h.card(C).decision).toBeNull();
  expect(h.card(F).decision?.kind).toBe('deny');
  expect(h.card(C).command).not.toBeNull();
  expect(h.card(F).command).toBeNull();
});

it('has no action broader than one decision, and asks nothing once disposed', async () => {
  vi.useFakeTimers();
  const h = await showing();
  // Two reads and two decisions are all a client has, and all the controller offers:
  // nothing that remembers, allows always, trusts, edits or makes a rule.
  expect(Object.keys(h.client).sort()).toEqual([
    'approve',
    'deny',
    'list',
    'read',
  ]);
  const actions = Object.getOwnPropertyNames(ApprovalsController.prototype);
  expect(
    actions.filter((name) =>
      /always|remember|trust|rule|policy|allow|edit|amend|session/i.test(name),
    ),
  ).toEqual([]);
  const calls = h.calls();
  const published = h.published();
  h.controller.dispose();
  h.controller.observe(connected('20'));
  h.controller.setVisible(false);
  h.controller.setWatching(true);
  h.controller.refresh();
  h.controller.want(F);
  h.controller.decide(C, 'approve');
  h.controller.more(C, 0);
  h.controller.moreFiles(C);
  await vi.advanceTimersByTimeAsync(600_000);
  expect(h.calls()).toBe(calls);
  expect(h.published()).toBe(published);
  expect(vi.getTimerCount()).toBe(0);
});
