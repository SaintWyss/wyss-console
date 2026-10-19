import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  type Changes,
  type InventoryReply,
  parseInventoryReply,
} from '../src/lib/contracts/configuration';
import {
  ConfigurationController,
  type ConfigurationClient,
  type ConfigurationState,
} from '../src/lib/features/configuration/controller';
import { ConfigurationCallError } from '../src/lib/desktop/configuration-client';
import fixture from './fixtures/configuration-inventory-v1.json';

const EVERY = 1500;
const base = (): InventoryReply => parseInventoryReply(fixture);

class FakeDesktop {
  revision = 1;
  reply: InventoryReply = base();
  calls: string[] = [];
  nextChange: Changes | null = null;
  failInventory: 'unavailable' | 'internal' | null = null;
  failChanges = false;
  hold: (() => void)[] | null = null;
  client(): ConfigurationClient {
    return {
      inventory: async () => {
        this.calls.push('inventory');
        if (this.hold) await new Promise<void>((r) => this.hold!.push(r));
        if (this.failInventory)
          throw new ConfigurationCallError(this.failInventory);
        return structuredClone({ ...this.reply, revision: this.revision });
      },
      changes: async (since) => {
        this.calls.push(`changes:${since}`);
        if (this.failChanges) throw new ConfigurationCallError('internal');
        return (
          this.nextChange ??
          (since === this.revision
            ? { result: 'same', revision: since }
            : { result: 'resync', revision: this.revision })
        );
      },
    };
  }
}

let desktop: FakeDesktop;
let states: ConfigurationState[];
let controller: ConfigurationController;
const settle = async () => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};
const last = () => states[states.length - 1];

beforeEach(() => {
  vi.useFakeTimers();
  desktop = new FakeDesktop();
  states = [];
  controller = new ConfigurationController(
    (s) => states.push(s),
    desktop.client(),
    EVERY,
  );
});
afterEach(() => {
  controller.dispose();
  vi.useRealTimers();
});

describe('the configuration controller', () => {
  it('reads nothing until its view is shown, and nothing again once it is hidden', async () => {
    await vi.advanceTimersByTimeAsync(10 * EVERY);
    expect(desktop.calls).toEqual([]);
    controller.setVisible(true);
    await settle();
    expect(desktop.calls).toEqual(['inventory']);
    controller.setVisible(false);
    await vi.advanceTimersByTimeAsync(10 * EVERY);
    expect(desktop.calls).toEqual(['inventory']);
  });

  it('shows loading, then the inventory', async () => {
    controller.setVisible(true);
    expect(last().phase).toBe('loading');
    await settle();
    expect(last().phase).toBe('ready');
    expect(last().reply?.inventory.items).toHaveLength(13);
  });

  it('asks only whether anything changed, at its pace, and does not read again if not', async () => {
    controller.setVisible(true);
    await settle();
    await vi.advanceTimersByTimeAsync(3 * EVERY + 10);
    expect(desktop.calls).toEqual([
      'inventory',
      'changes:1',
      'changes:1',
      'changes:1',
    ]);
  });

  it('reads again when it is told something changed, and shows the new revision', async () => {
    controller.setVisible(true);
    await settle();
    desktop.revision = 2;
    desktop.nextChange = {
      result: 'changed',
      revision: 2,
      sources: ['claude_user_settings'],
    };
    await vi.advanceTimersByTimeAsync(EVERY + 10);
    desktop.nextChange = null;
    expect(desktop.calls.slice(0, 3)).toEqual([
      'inventory',
      'changes:1',
      'inventory',
    ]);
    expect(last().reply?.revision).toBe(2);
  });

  it('reads again after a resync, which is what a restart of the desktop looks like', async () => {
    controller.setVisible(true);
    await settle();
    desktop.revision = 1;
    desktop.nextChange = { result: 'resync', revision: 1 };
    await vi.advanceTimersByTimeAsync(EVERY + 10);
    expect(desktop.calls).toContain('inventory');
    expect(desktop.calls.filter((c) => c === 'inventory')).toHaveLength(2);
  });

  it('reads once at a time', async () => {
    desktop.hold = [];
    controller.setVisible(true);
    controller.refresh();
    controller.refresh();
    await settle();
    expect(desktop.calls).toEqual(['inventory']);
    desktop.hold.forEach((release) => release());
    await settle();
  });

  it('says unavailable, not empty, when the desktop has nothing to read from', async () => {
    desktop.failInventory = 'unavailable';
    controller.setVisible(true);
    await settle();
    expect(last().phase).toBe('unavailable');
    expect(last().reply).toBeNull();
    // It does not keep asking a desktop that said it cannot.
    await vi.advanceTimersByTimeAsync(5 * EVERY);
    expect(desktop.calls).toEqual(['inventory']);
    // Until the user asks again.
    desktop.failInventory = null;
    controller.refresh();
    await settle();
    expect(last().phase).toBe('ready');
  });

  it('keeps what it showed, marked old, when a later read fails', async () => {
    controller.setVisible(true);
    await settle();
    desktop.failChanges = true;
    await vi.advanceTimersByTimeAsync(EVERY + 10);
    expect(last().stale).toBe(true);
    expect(last().reply).not.toBeNull();
    desktop.failChanges = false;
    await vi.advanceTimersByTimeAsync(EVERY + 10);
    await settle();
    expect(last().stale).toBe(false);
  });

  it('shows a failed first read as failed and offers another', async () => {
    desktop.failInventory = 'internal';
    controller.setVisible(true);
    await settle();
    expect(last().phase).toBe('failed');
    desktop.failInventory = null;
    controller.refresh();
    await settle();
    expect(last().phase).toBe('ready');
  });

  it('closes the details of an item that is no longer there', async () => {
    controller.setVisible(true);
    await settle();
    const id = last().reply!.inventory.items[0].id;
    controller.select(id);
    expect(last().selected).toBe(id);
    desktop.reply = {
      ...desktop.reply,
      inventory: {
        ...desktop.reply.inventory,
        items: desktop.reply.inventory.items.slice(1),
      },
    };
    desktop.revision = 2;
    controller.refresh();
    await settle();
    expect(last().selected).toBeNull();
  });

  it('keeps filters and selection across reads, and stops for good when disposed', async () => {
    controller.setVisible(true);
    await settle();
    controller.setProvider('codex');
    controller.setKind('skill');
    controller.refresh();
    await settle();
    expect(last().provider).toBe('codex');
    expect(last().kind).toBe('skill');
    controller.dispose();
    const seen = states.length;
    await vi.advanceTimersByTimeAsync(5 * EVERY);
    expect(states.length).toBe(seen);
  });
});
