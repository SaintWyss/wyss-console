import type { SyncStatus } from '../../contracts/arsenal';
import { msg, type Phrase } from '../../i18n/msg';
import type { StateMark } from './catalogue';

export interface SyncSummary extends StateMark {
  label: Phrase;
  text: Phrase;
}

/** Counted words: the English keeps "1 change" and "2 changes" as two sentences. */
const COUNTED = {
  change: { one: msg('{count} change'), many: msg('{count} changes') },
  conflict: { one: msg('{count} conflict'), many: msg('{count} conflicts') },
  item: { one: msg('{count} item'), many: msg('{count} items') },
} as const;

function count(n: number, word: keyof typeof COUNTED): Phrase {
  const forms = COUNTED[word];
  return { text: n === 1 ? forms.one : forms.many, values: { count: n } };
}

/** Several phrases as one list, separated by "; ". */
function listed(parts: Phrase[]): Phrase {
  const [first, ...rest] = parts;
  if (rest.length === 0) return first!;
  return {
    text: msg('{first}; {rest}'),
    values: { first: first!, rest: listed(rest) },
  };
}

/** Notes this recent still ask for attention; older ones stay in the details only. */
const RECENT_SECONDS = 24 * 60 * 60;

/**
 * One line for the state of synchronization. It says what this computer can know: what it
 * published, what each other computer acknowledged applying, and when it last heard from it.
 * A file this computer wrote is never presented as applied elsewhere.
 */
export function syncSummary(sync: SyncStatus, now: number): SyncSummary {
  const recent = (at: number) => now - at < RECENT_SECONDS;
  if (sync.error)
    return {
      tone: 'error',
      symbol: '✕',
      label: { text: msg('Sync error') },
      text: {
        text: msg('The last run failed: {error}.'),
        values: { error: sync.error },
      },
    };
  if (sync.pending.length > 0)
    return {
      tone: 'pending',
      symbol: '●',
      label: { text: msg('Receiving') },
      text: {
        text: msg('{changes} waiting for files still on their way.'),
        values: { changes: count(sync.pending.length, 'change') },
      },
    };
  const conflicts = sync.conflicts.filter((n) => recent(n.at)).length;
  const rejected = sync.rejected.filter((n) => recent(n.at)).length;
  const removed = sync.removed.filter((n) => recent(n.at)).length;
  if (conflicts > 0 || rejected > 0 || removed > 0) {
    const parts: Phrase[] = [];
    if (conflicts > 0)
      parts.push({
        text: msg('{conflicts} kept both versions'),
        values: { conflicts: count(conflicts, 'conflict') },
      });
    if (rejected > 0)
      parts.push({
        text: msg('{items} refused'),
        values: { items: count(rejected, 'item') },
      });
    if (removed > 0)
      parts.push({
        text: msg('{items} deleted on another computer, a copy kept here'),
        values: { items: count(removed, 'item') },
      });
    return {
      tone: 'warn',
      symbol: '!',
      label: { text: msg('Needs a look') },
      text: {
        text: msg('{parts}. See the details.'),
        values: { parts: listed(parts) },
      },
    };
  }
  if (sync.peers.length === 0)
    return {
      tone: 'neutral',
      symbol: '–',
      label: { text: msg('This computer only') },
      text: {
        text: msg('No other computer has written to the shared folder yet.'),
      },
    };
  const behind = sync.peers.filter((p) => p.applied_mine < p.published_mine);
  const hosts = sync.peers.map((p) => p.host).join(', ');
  const first = behind[0];
  if (first) {
    const hostsBehind = behind.map((p) => p.host).join(', ');
    const changes = count(
      Math.max(...behind.map((p) => p.published_mine - p.applied_mine)),
      'change',
    );
    return {
      tone: 'pending',
      symbol: '●',
      label: { text: msg('Published') },
      text: first.last_heard
        ? {
            text: msg(
              '{hosts} has not applied {changes} from here yet (last heard {time}).',
            ),
            values: {
              hosts: hostsBehind,
              changes,
              time: ago(first.last_heard, now),
            },
          }
        : {
            text: msg('{hosts} has not applied {changes} from here yet.'),
            values: { hosts: hostsBehind, changes },
          },
    };
  }
  return {
    tone: 'ok',
    symbol: '✓',
    label: { text: msg('In sync') },
    text: sync.last_success
      ? {
          text: msg(
            '{hosts} applied everything published here; checked {when}.',
          ),
          values: { hosts, when: ago(sync.last_success, now) },
        }
      : {
          text: msg('{hosts} applied everything published here.'),
          values: { hosts },
        },
  };
}

export function ago(at: number, now: number): Phrase {
  const seconds = Math.max(0, now - at);
  if (seconds < 60) return { text: msg('just now') };
  if (seconds < 3600)
    return {
      text: msg('{minutes} min ago'),
      values: { minutes: Math.floor(seconds / 60) },
    };
  if (seconds < 86400)
    return {
      text: msg('{hours} h ago'),
      values: { hours: Math.floor(seconds / 3600) },
    };
  return {
    text: msg('{days} d ago'),
    values: { days: Math.floor(seconds / 86400) },
  };
}
