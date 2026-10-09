import {
  parseManagedApprovalId,
  parseManagedSessionId,
  parseManagedTurnId,
  type ManagedSessionId,
  type ManagedTurnId,
} from '../src/lib/contracts/identities';
import type { SessionFailure } from '../src/lib/contracts/sessions';
import {
  turnEnded,
  type TurnEventKind,
  type TurnList,
  type TurnReceipt,
  type TurnState,
  type TurnSummary,
  type TurnView,
} from '../src/lib/contracts/turns';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import {
  AgentController,
  type AgentState,
  type TurnsClient,
} from '../src/lib/features/agent/controller';
import { connected, settle, type Deferred } from './sessions-harness';

export const S = parseManagedSessionId('22222222-2222-4222-8222-222222222222');
export const S2 = parseManagedSessionId('44444444-4444-4444-8444-444444444444');
const turnId = (digit: string) =>
  parseManagedTurnId(
    `${digit.repeat(8)}-${digit.repeat(4)}-4${digit.repeat(3)}-8${digit.repeat(3)}-${digit.repeat(12)}`,
  );
export const [T1, T2, T3] = ['a', 'b', 'c'].map(turnId);
export const T9 = turnId('9');
/** The handle of an approval a turn waits on. */
export const A1 = parseManagedApprovalId(
  'dddddddd-dddd-4ddd-8ddd-dddddddddddd',
);

const REASON = {
  completed: 'completed',
  interrupted: 'user_cancel',
  failed: 'provider_failed',
  indeterminate: 'authority_restarted',
} as const;
/** A turn of `S` as the authority would hold it in `state`. */
export function turn(
  id: ManagedTurnId,
  state: TurnState,
  revision = 1,
  over: Partial<TurnSummary> = {},
): TurnSummary {
  const ended = turnEnded(state);
  return {
    id,
    session: S,
    state,
    revision,
    started_at: '2026-10-09T10:00:00Z',
    updated_at: '2026-10-09T10:00:09Z',
    ended_at: ended ? '2026-10-09T10:00:09Z' : null,
    reason: ended ? REASON[state as keyof typeof REASON] : null,
    output: true,
    cancellable: !ended,
    ...over,
  };
}
/** The turns of a session, newest first, attached and at head nine unless said. */
export function listing(
  turns: TurnSummary[],
  over: Omit<Partial<TurnList>, 'session'> & {
    session?: Partial<TurnList['session']>;
  } = {},
): TurnList {
  const { session, ...rest } = over;
  return {
    schema_version: 2,
    head: '9',
    turns,
    runnable: turns.every((t) => turnEnded(t.state)),
    partial: false,
    ...rest,
    session: {
      id: S,
      state: 'prepared',
      revision: 1,
      binding: 'bound_attached',
      workspace: '/home/dev/rag-corp',
      mode: 'inference',
      ...session,
    },
  };
}
export type Told = [number, TurnEventKind, (string | null)?];
/** One reading: `events` as `[position, kind, text]`, ending where they end. */
export function reading(
  of: TurnSummary,
  events: Told[],
  over: Partial<TurnView> = {},
): TurnView {
  return {
    schema_version: 2,
    turn: of,
    events: events.map(([n, kind, text]) => ({
      sequence: String(n),
      kind,
      text: text ?? null,
    })),
    next: events.length ? String(events.at(-1)![0]) : null,
    more: false,
    gap: false,
    approval: null,
    ...over,
  };
}

/** A client whose every call waits to be answered by the test. */
export function harness() {
  const lists: (Deferred<TurnList> & { session: ManagedSessionId })[] = [];
  const reads: (Deferred<TurnView> & {
    turn: ManagedTurnId;
    after: string | null;
    wait: boolean;
  })[] = [];
  const starts: (Deferred<TurnReceipt> & { args: unknown[] })[] = [];
  const cancels: (Deferred<TurnReceipt> & { args: unknown[] })[] = [];
  /** Readings asked for and not yet answered, and the most there ever were at once. */
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
  const client: TurnsClient = {
    turns(session) {
      const d = deferred<TurnList>();
      lists.push({ ...d, session });
      return d.promise;
    },
    read(of, after, wait) {
      const d = deferred<TurnView>();
      flight.now++;
      flight.most = Math.max(flight.most, flight.now);
      const landed = () => {
        flight.now--;
      };
      void d.promise.then(landed, landed);
      reads.push({ ...d, turn: of, after, wait });
      return d.promise;
    },
    start(...args) {
      const d = deferred<TurnReceipt>();
      starts.push({ ...d, args });
      return d.promise;
    },
    cancel(...args) {
      const d = deferred<TurnReceipt>();
      cancels.push({ ...d, args });
      return d.promise;
    },
  };
  let state!: AgentState;
  let minted = 0;
  let published = 0;
  const controller = new AgentController(
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
    starts,
    cancels,
    flight,
    state: () => state,
    /** The session that is shown. The tests always have one open. */
    shown: () => state.current!,
    published: () => published,
    calls: () => lists.length + reads.length + starts.length + cancels.length,
  };
}
/**
 * `S` open in the shown Agent view, connected at head nine, its first list answered
 * with `turns`.
 */
export async function showing(turns: TurnSummary[] = [], over = {}) {
  const h = harness();
  h.controller.open(S, 'rag-corp');
  h.controller.setVisible(true);
  h.controller.observe(connected('9'));
  h.lists[0].resolve(listing(turns, over));
  await settle();
  return h;
}
/** A turn started from the composer and accepted as `id`, its forced list answered. */
export async function started(
  h: ReturnType<typeof harness>,
  id: ManagedTurnId = T1,
  prompt = 'Say hello.',
  earlier: TurnSummary[] = [],
) {
  h.controller.setDraft(prompt);
  h.controller.start();
  h.starts.at(-1)!.resolve({ id });
  await settle();
  h.lists.at(-1)!.resolve(listing([turn(id, 'starting'), ...earlier]));
  await settle();
}
