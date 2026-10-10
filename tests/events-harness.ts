import {
  EVENT_KIND_CATEGORY,
  parseEventsPage,
  type EventFacts,
  type EventKind,
  type EventsPage,
} from '../src/lib/contracts/events';

export const STREAM = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
export const OTHER_STREAM = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';
export const S1 = '22222222-2222-4222-8222-222222222222';
export const S2 = '44444444-4444-4444-8444-444444444444';
export const T1 = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1';
export const T2 = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb2';
export const A1 = 'ffffffff-ffff-4fff-8fff-ffffffffffff';

export const NO_FACTS: EventFacts = {
  turn_reason: null,
  approval_kind: null,
  approval_reason: null,
  approval_effect: null,
  desired: null,
  exit: null,
  expected: null,
  runtime_version: null,
  scopes: null,
  expires_at: null,
  revocation: null,
};

export interface RawRow {
  sequence: string;
  at: string;
  category: string;
  kind: string;
  disposition: string;
  security: boolean;
  session: string | null;
  turn: string | null;
  approval: string | null;
  facts: Record<string, unknown>;
}
/** One row as the bridge sends it. A turn event of session one unless said otherwise. */
export function row(
  sequence: number | string,
  kind: EventKind = 'turn_running',
  over: Partial<Omit<RawRow, 'facts'>> & { facts?: Partial<EventFacts> } = {},
): RawRow {
  const category = EVENT_KIND_CATEGORY[kind];
  const { facts, ...rest } = over;
  const position = BigInt(sequence);
  return {
    sequence: String(sequence),
    at: `2026-10-09T10:${String(Number(position % 60n)).padStart(2, '0')}:00Z`,
    category,
    kind,
    disposition: 'normal',
    security: category === 'lease',
    session: category === 'runtime' || category === 'authority' ? null : S1,
    turn: category === 'turn' || category === 'approval' ? T1 : null,
    approval: category === 'approval' ? A1 : null,
    ...rest,
    facts: { ...NO_FACTS, ...facts },
  };
}
export interface RawPage {
  schema_version: number;
  stream: string;
  head: string | null;
  events: RawRow[];
  older: string | null;
  gap: boolean;
}
/** The window of `count` events that ends at `newest`, as the bridge sends it. */
export function rawPage(
  newest: number,
  count: number,
  over: Partial<RawPage> & { kind?: (at: number) => EventKind } = {},
): RawPage {
  const { kind, ...rest } = over;
  const events = Array.from({ length: count }, (_, i) =>
    row(newest - i, kind ? kind(newest - i) : 'turn_running'),
  );
  const oldest = newest - count + 1;
  const gap = rest.gap ?? false;
  return {
    schema_version: 1,
    stream: STREAM,
    head: newest > 0 ? String(newest) : null,
    events,
    older: count > 0 && oldest > 1 && !gap ? String(oldest) : null,
    gap,
    ...rest,
  };
}
export const page = (...args: Parameters<typeof rawPage>): EventsPage =>
  parseEventsPage(rawPage(...args));
export const positions = (events: readonly { sequence: string }[]) =>
  events.map((event) => event.sequence);
/** The positions from `newest` down to `oldest`, as text. */
export const run = (newest: number, oldest: number) =>
  Array.from({ length: newest - oldest + 1 }, (_, i) => String(newest - i));
