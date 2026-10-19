import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { parseInventoryReply } from '../src/lib/contracts/configuration';
import type { ConfigurationClient } from '../src/lib/features/configuration/controller';
import ConfigurationHarness from './ConfigurationHarness.svelte';
import fixture from './fixtures/configuration-inventory-v1.json';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
});
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
  flushSync();
};

async function shown(client?: Partial<ConfigurationClient>) {
  const reply = parseInventoryReply(fixture);
  component = mount(ConfigurationHarness, {
    target: document.body,
    props: {
      client: {
        inventory: async () => reply,
        changes: async (since: number) => ({ result: 'same', revision: since }),
        ...client,
      } as ConfigurationClient,
    },
  });
  await settle();
}

describe('the configuration view', () => {
  it('lists what the agents are set up with, read-only, and says so', async () => {
    await shown();
    const root = document.querySelector('.configuration-feature')!;
    expect(text(root.querySelector('.sessions-count'))).toBe(
      '13 items from 17 places',
    );
    expect(text(root)).toContain('It is read-only');
    expect(
      root.querySelectorAll('ul[aria-label="Configuration items"] li'),
    ).toHaveLength(13);
    // Nothing on the page changes a file: the only controls choose what to look at.
    const controls = [...root.querySelectorAll('button')].map((b) => text(b));
    expect(
      controls.some((c) =>
        /save|edit|delete|remove|enable|disable|apply|move/i.test(c),
      ),
    ).toBe(false);
  });

  it('never shows a secret or a path', async () => {
    await shown();
    const page = document.body.textContent ?? '';
    expect(page).not.toContain('SEKRET');
    expect(page).not.toMatch(/\/home\/|\/etc\/|\/tmp\//);
  });

  it('narrows by agent and by kind', async () => {
    await shown();
    const chip = (name: string) =>
      [...document.querySelectorAll('button.chip')].find((b) =>
        text(b).startsWith(name),
      ) as HTMLButtonElement;
    chip('Codex').click();
    await settle();
    const rows = () => [...document.querySelectorAll('.configuration-row')];
    expect(rows().length).toBeGreaterThan(0);
    expect(rows().every((r) => text(r).startsWith('Codex'))).toBe(true);
    chip('Skills').click();
    await settle();
    expect(rows().every((r) => text(r).includes('Skill'))).toBe(true);
    expect(chip('Codex').getAttribute('aria-pressed')).toBe('true');
  });

  it('opens an item to say where it is from, what the file says and why it cannot be changed', async () => {
    await shown();
    const server = [...document.querySelectorAll('.configuration-row')].find(
      (r) => text(r).includes('context7'),
    ) as HTMLButtonElement;
    expect(text(server)).toContain('values hidden');
    server.click();
    await settle();
    const detail = document.querySelector('.configuration-detail')!;
    expect(text(detail)).toContain('Account file (MCP servers only)');
    expect(text(detail)).toContain('On in the file');
    expect(text(detail)).toContain(
      'Cannot be confirmed without running the provider',
    );
    expect(text(detail)).toContain('changed only through its own command');
    expect(text(detail)).toContain('hidden (environment value)');
    server.click();
    await settle();
    expect(document.querySelector('.configuration-detail')).toBeNull();
  });

  it('says what became of each place it looked, and counts the ones that went wrong', async () => {
    await shown();
    const sources = document.querySelector('.configuration-sources')!;
    expect(text(sources.querySelector('summary'))).toContain('1 problem');
    const list = text(sources);
    expect(list).toContain('reading stopped at line 1, column 3');
    expect(list).toContain(
      'Codex loads a project’s own configuration only for a project you trust',
    );
    expect(list).toContain(
      'Its folder is shared with a group, so it is only read.',
    );
  });

  it('tells an unavailable desktop from an empty one', async () => {
    const { ConfigurationCallError } =
      await import('../src/lib/desktop/configuration-client');
    await shown({
      inventory: async () => {
        throw new ConfigurationCallError('unavailable');
      },
    });
    const page = text(document.querySelector('.configuration-feature'));
    expect(page).toContain('There is no configuration to read here');
    expect(page).not.toContain('No hooks, skills');
  });

  it('is hidden and inert when another view is shown', async () => {
    await shown();
    const root = document.querySelector(
      '.configuration-feature',
    ) as HTMLElement;
    expect(root.hidden).toBe(false);
    expect(root.inert).toBe(false);
  });
});
