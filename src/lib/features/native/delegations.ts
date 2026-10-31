import type { Delegation } from '../../desktop/native-client';
import type { StateMark, StateTone } from '../arsenal/catalogue';

/** How a delegated agent's status is drawn: tone and symbol, beside its word. */
export function delegationMark(status: string): StateMark {
  switch (status) {
    case 'running':
      return { tone: 'pending', symbol: '●' };
    case 'succeeded':
      return { tone: 'ok', symbol: '✓' };
    case 'cancelled':
      return { tone: 'neutral', symbol: '–' };
    case 'failed':
    case 'timed_out':
    case 'interrupted':
      return { tone: 'error', symbol: '✕' };
    default:
      return { tone: 'neutral', symbol: '?' };
  }
}

export interface DelegationGroup {
  /** Unique per parent: its provider and its session. */
  key: string;
  parent: string;
  provider: string;
  items: Delegation[];
}

/**
 * The delegations under the session that made them, in the order the host listed them: the
 * first delegation of a parent places its group; a parent's agents stay together. A parent is
 * its provider and its session, never the session id alone.
 */
export function delegationGroups(
  delegations: readonly Delegation[],
): DelegationGroup[] {
  const groups = new Map<string, DelegationGroup>();
  for (const d of delegations) {
    const key = `${d.parent_provider}:${d.parent_session}`;
    const group = groups.get(key);
    if (group) group.items.push(d);
    else
      groups.set(key, {
        key,
        parent: d.parent_session,
        provider: d.parent_provider,
        items: [d],
      });
  }
  return [...groups.values()];
}

export interface TallyEntry {
  tone: StateTone;
  symbol: string;
  label: string;
  count: number;
}

const TALLY: { tone: StateTone; label: string }[] = [
  { tone: 'pending', label: 'running' },
  { tone: 'ok', label: 'done' },
  { tone: 'error', label: 'failed' },
  { tone: 'neutral', label: 'cancelled' },
];

/** How many are running, done, failed and cancelled. A count of zero is left out. */
export function delegationTally(
  delegations: readonly Delegation[],
): TallyEntry[] {
  return TALLY.flatMap(({ tone, label }) => {
    const marks = delegations
      .map((d) => delegationMark(d.status))
      .filter((m) => m.tone === tone);
    return marks.length > 0
      ? [{ tone, label, symbol: marks[0]!.symbol, count: marks.length }]
      : [];
  });
}
