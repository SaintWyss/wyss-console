import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import SyncLine from '../src/lib/features/arsenal/SyncLine.svelte';
import { syncSummary } from '../src/lib/features/arsenal/sync-present';
import { ArsenalController } from '../src/lib/features/arsenal/controller';
import { parseSyncStatus, type SyncStatus } from '../src/lib/contracts/arsenal';
import type { ArsenalClient } from '../src/lib/desktop/arsenal-client';

const NOW = 2_000_000_000;
const status = (over: Partial<SyncStatus> = {}): SyncStatus => ({
  enabled: true,
  folder: '~/.local/share/wyss-console-sync/arsenal',
  device: 'a'.repeat(32),
  host: 'saint-acer',
  generation: 1,
  last_run: NOW - 30,
  last_success: NOW - 30,
  error: null,
  published: 4,
  pending: [],
  conflicts: [],
  rejected: [],
  peers: [
    {
      device: 'b'.repeat(32),
      host: 'Santiago-PC',
      last_heard: NOW - 120,
      applied_mine: 4,
      published_mine: 4,
    },
  ],
  ...over,
});

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
  vi.useRealTimers();
});

it('says in sync only when the other computer acknowledged applying everything', () => {
  expect(syncSummary(status(), NOW)).toMatchObject({
    tone: 'ok',
    label: 'In sync',
  });
  const behind = syncSummary(
    status({ peers: [{ ...status().peers[0]!, applied_mine: 1 }] }),
    NOW,
  );
  expect(behind.tone).toBe('pending');
  expect(behind.label).toBe('Published');
  expect(behind.text).toContain('Santiago-PC has not applied 3 changes');
  expect(behind.text).not.toMatch(/applied everything/);
  expect(syncSummary(status({ peers: [] }), NOW).label).toBe(
    'This computer only',
  );
});

it('puts errors, waiting changes and recent conflicts before anything else', () => {
  const note = {
    at: NOW - 60,
    kind: 'preset',
    name: 'Frontend',
    detail: 'kept both',
  };
  expect(syncSummary(status({ error: 'library: busy' }), NOW).tone).toBe(
    'error',
  );
  expect(syncSummary(status({ pending: [note] }), NOW).label).toBe('Receiving');
  const conflict = syncSummary(status({ conflicts: [note] }), NOW);
  expect(conflict).toMatchObject({ tone: 'warn', label: 'Needs a look' });
  expect(conflict.text).toContain('1 conflict kept both versions');
  // An old conflict stays in the details, not in the headline.
  expect(
    syncSummary(status({ conflicts: [{ ...note, at: NOW - 3 * 86400 }] }), NOW)
      .tone,
  ).toBe('ok');
  // Every state has a word and a symbol, not a color alone.
  for (const s of [
    status(),
    status({ peers: [] }),
    status({ pending: [note] }),
  ]) {
    const summary = syncSummary(s, NOW);
    expect(summary.label.length).toBeGreaterThan(0);
    expect(summary.symbol.trim().length).toBeGreaterThan(0);
  }
});

it('shows the line with its details and asks for a run on request', () => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW * 1000);
  const onsyncnow = vi.fn();
  component = mount(SyncLine, {
    target: document.body,
    props: {
      sync: status({
        rejected: [
          {
            at: NOW - 10,
            kind: 'capability',
            name: 'leaky',
            detail: 'SKILL.md (may_hold_secret) (from Santiago-PC)',
          },
        ],
      }),
      onsyncnow,
    },
  });
  flushSync();
  const text = document.body.textContent?.replace(/\s+/g, ' ') ?? '';
  expect(text).toContain('Needs a look');
  expect(text).toContain('Santiago-PC');
  expect(text).toContain('applied 4 of 4 change(s) from here');
  expect(text).toContain('leaky');
  expect(document.querySelector('[role="status"]')).not.toBeNull();
  [...document.querySelectorAll('button')]
    .find((b) => b.textContent === 'Check now')!
    .click();
  expect(onsyncnow).toHaveBeenCalledOnce();
});

it('reads the library again when synchronization changed it, and only then', async () => {
  vi.useFakeTimers();
  let generation = 1;
  const capabilities = vi.fn(async () => []);
  const client = {
    capabilities,
    presets: vi.fn(async () => []),
    selections: vi.fn(async () => []),
    syncStatus: vi.fn(async () => status({ generation })),
  } as unknown as ArsenalClient;
  const controller = new ArsenalController(() => undefined, client);
  controller.setVisible(true);
  await vi.advanceTimersByTimeAsync(10);
  const reads = capabilities.mock.calls.length;
  await vi.advanceTimersByTimeAsync(4000);
  expect(capabilities.mock.calls.length).toBe(reads);
  generation = 2;
  await vi.advanceTimersByTimeAsync(4000);
  expect(capabilities.mock.calls.length).toBe(reads + 1);
  expect(controller.snapshot().sync?.generation).toBe(2);
  // Hidden: no more reads.
  controller.setVisible(false);
  const polls = (client.syncStatus as ReturnType<typeof vi.fn>).mock.calls
    .length;
  await vi.advanceTimersByTimeAsync(20_000);
  expect(
    (client.syncStatus as ReturnType<typeof vi.fn>).mock.calls.length,
  ).toBe(polls);
  controller.dispose();
});

it('does not take the desktop’s status on faith', () => {
  expect(() => parseSyncStatus({ ...status(), peers: 'x' })).toThrow();
  expect(() => parseSyncStatus({ ...status(), generation: '1' })).toThrow();
  expect(parseSyncStatus(status()).peers[0]!.host).toBe('Santiago-PC');
});
