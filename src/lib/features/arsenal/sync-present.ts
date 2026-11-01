import type { SyncPeer, SyncStatus } from '../../contracts/arsenal';
import type { StateMark } from './catalogue';

export interface SyncSummary extends StateMark {
  label: string;
  text: string;
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
      label: 'Sync error',
      text: `The last run failed: ${sync.error}.`,
    };
  if (sync.pending.length > 0)
    return {
      tone: 'pending',
      symbol: '●',
      label: 'Receiving',
      text: `${count(sync.pending.length, 'change')} waiting for files still on their way.`,
    };
  const conflicts = sync.conflicts.filter((n) => recent(n.at)).length;
  const rejected = sync.rejected.filter((n) => recent(n.at)).length;
  const removed = sync.removed.filter((n) => recent(n.at)).length;
  if (conflicts > 0 || rejected > 0 || removed > 0) {
    const parts = [
      conflicts > 0 ? `${count(conflicts, 'conflict')} kept both versions` : '',
      rejected > 0 ? `${count(rejected, 'item')} refused` : '',
      removed > 0
        ? `${count(removed, 'item')} deleted on another computer, a copy kept here`
        : '',
    ].filter(Boolean);
    return {
      tone: 'warn',
      symbol: '!',
      label: 'Needs a look',
      text: `${parts.join('; ')}. See the details.`,
    };
  }
  if (sync.peers.length === 0)
    return {
      tone: 'neutral',
      symbol: '–',
      label: 'This computer only',
      text: 'No other computer has written to the shared folder yet.',
    };
  const behind = sync.peers.filter((p) => p.applied_mine < p.published_mine);
  if (behind.length > 0)
    return {
      tone: 'pending',
      symbol: '●',
      label: 'Published',
      text: `${behind.map((p) => p.host).join(', ')} has not applied ${count(
        Math.max(...behind.map((p) => p.published_mine - p.applied_mine)),
        'change',
      )} from here yet${lastHeard(behind[0]!, now)}.`,
    };
  return {
    tone: 'ok',
    symbol: '✓',
    label: 'In sync',
    text: `${sync.peers.map((p) => p.host).join(', ')} applied everything published here${
      sync.last_success ? `; checked ${ago(sync.last_success, now)}` : ''
    }.`,
  };
}

function lastHeard(peer: SyncPeer, now: number): string {
  return peer.last_heard ? ` (last heard ${ago(peer.last_heard, now)})` : '';
}

export function ago(at: number, now: number): string {
  const seconds = Math.max(0, now - at);
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

function count(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}
