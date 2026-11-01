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

/** Each status counted under its own word; tones only color them. */
const TALLY: { status: string; label: string }[] = [
  { status: 'running', label: 'running' },
  { status: 'succeeded', label: 'done' },
  { status: 'failed', label: 'failed' },
  { status: 'timed_out', label: 'timed out' },
  { status: 'interrupted', label: 'interrupted' },
  { status: 'cancelled', label: 'cancelled' },
];
const KNOWN = new Set(TALLY.map((entry) => entry.status));

/** How many are in each status. A count of zero is left out; a status the window does not
 *  know is counted as "other", never folded into another one. */
export function delegationTally(
  delegations: readonly Delegation[],
): TallyEntry[] {
  const count = (match: (status: string) => boolean) =>
    delegations.filter((d) => match(d.status)).length;
  const entries = TALLY.map(({ status, label }) => ({
    ...delegationMark(status),
    label,
    count: count((s) => s === status),
  }));
  entries.push({
    ...delegationMark('unknown'),
    label: 'other',
    count: count((s) => !KNOWN.has(s)),
  });
  return entries.filter((entry) => entry.count > 0);
}
