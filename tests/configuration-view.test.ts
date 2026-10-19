import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { parseInventoryReply } from '../src/lib/contracts/configuration';
import type { ConfigurationClient } from '../src/lib/features/configuration/controller';
import {
  FakeChanges,
  fixture as changesFixture,
} from './configuration-changes-harness';
const fixtureHistory = () => changesFixture.history();
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

async function shown(
  client?: Partial<ConfigurationClient>,
  changes: FakeChanges = new FakeChanges(),
) {
  const reply = parseInventoryReply(fixture);
  component = mount(ConfigurationHarness, {
    target: document.body,
    props: {
      client: {
        inventory: async () => reply,
        changes: async (since: number) => ({ result: 'same', revision: since }),
        ...client,
      } as ConfigurationClient,
      changesClient: changes.client(),
    },
  });
  await settle();
  return changes;
}

describe('the configuration view', () => {
  it('lists what the agents are set up with, read-only, and says so', async () => {
    await shown();
    const root = document.querySelector('.configuration-feature')!;
    expect(text(root.querySelector('.sessions-count'))).toBe(
      '13 items from 17 places',
    );
    expect(text(root)).toContain('Nothing changes until you confirm a plan');
    expect(
      root.querySelectorAll('ul[aria-label="Configuration items"] li'),
    ).toHaveLength(13);
    // No control on the page applies anything before a plan is on screen to confirm.
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

const click = async (element: Element | null | undefined) => {
  expect(element).toBeTruthy();
  (element as HTMLElement).click();
  await settle();
};
const button = (label: string, root: ParentNode = document.body) =>
  [...root.querySelectorAll('button')].find((b) => text(b).startsWith(label));
const row = (name: string) =>
  [...document.querySelectorAll('.configuration-row')].find((r) =>
    text(r).includes(name),
  ) as HTMLButtonElement;

describe('changing, in the view', () => {
  it('offers a change for a Claude Code hook and not for Codex, a server or a rule', async () => {
    await shown();
    await click(row('PreToolUse'));
    expect(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    ).toBeTruthy();
    expect(text(document.querySelector('.configuration-detail'))).toContain(
      'You can do it here, with a plan you confirm',
    );
    await click(row('context7'));
    expect(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    ).toBeUndefined();
    expect(text(document.querySelector('.configuration-detail'))).toContain(
      'changed only through its own command',
    );
    // A Codex entry: read-only for now.
    await click(button('Codex'));
    const codexRow = [
      ...document.querySelectorAll('.configuration-row'),
    ][0] as HTMLButtonElement;
    await click(codexRow);
    expect(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    ).toBeUndefined();
  });

  it('shows the plan with what would change, how it is kept and the effect, and applies it on confirmation', async () => {
    const changes = await shown();
    await click(row('PreToolUse'));
    await click(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    );
    expect(changes.calls[0]).toMatch(/^plan:park_hook:/);
    const plan = document.querySelector('.configuration-plan')!;
    expect(text(plan)).toContain('Turn off this hook: PreToolUse · Bash');
    expect(text(plan.querySelector('.configuration-diff'))).toContain(
      'command: guard.sh',
    );
    expect(text(plan)).toContain(
      'encrypted, with a key held by your system’s credential store',
    );
    expect(text(plan)).toContain('cannot be put back from here');
    expect(text(plan)).toContain(
      'A new Claude Code session follows the file (checked)',
    );
    expect(text(plan)).toContain(
      'You will be asked to confirm in a separate window',
    );
    expect(button('Continue to confirm', plan)).toBeTruthy();
    await click(button('Continue to confirm', plan));
    expect(changes.calls.some((c) => c.startsWith('apply:'))).toBe(true);
    expect(document.querySelector('.configuration-plan')).toBeNull();
    expect(text(document.querySelector('.configuration-feature'))).toContain(
      'Done. The file was changed and read back.',
    );
  });

  it('cancels a plan without applying anything', async () => {
    const changes = await shown();
    await click(row('PreToolUse'));
    await click(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    );
    await click(
      button('Cancel', document.querySelector('.configuration-plan')!),
    );
    expect(document.querySelector('.configuration-plan')).toBeNull();
    expect(changes.calls.some((c) => c.startsWith('apply:'))).toBe(false);
  });

  it('says a hook holds hidden values without showing any, and a normal plan needs no other window', async () => {
    const changes = new FakeChanges();
    changes.plan = {
      ...changes.plan,
      holds_hidden_values: true,
      risk: 'normal',
      needs_native_dialog: false,
    };
    await shown(undefined, changes);
    await click(row('PreToolUse'));
    await click(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    );
    const plan = document.querySelector('.configuration-plan')!;
    expect(text(plan)).toContain(
      'This entry holds values that are not shown here',
    );
    expect(button('Apply', plan)).toBeTruthy();
    expect(text(plan)).not.toContain('separate window');
  });

  it('says in words why a change could not be planned', async () => {
    const changes = new FakeChanges();
    changes.failNext = 'key_store_unavailable';
    await shown(undefined, changes);
    await click(row('PreToolUse'));
    await click(
      button('Turn off…', document.querySelector('.configuration-detail')!),
    );
    expect(text(document.querySelector('.configuration-feature'))).toContain(
      'credential store is not available, and this entry may hold a secret',
    );
    expect(document.querySelector('.configuration-plan')).toBeNull();
  });

  it('turns every hook off with one plan', async () => {
    const changes = await shown();
    await click(button('Turn off every hook'));
    expect(changes.calls[0]).toBe(
      'plan:disable_all_hooks:claude_user_settings',
    );
  });

  it('lists the history without content and undoes only what can be undone', async () => {
    const changes = await shown();
    await click(button('History'));
    const list = document.querySelector('ul[aria-label="Changes"]')!;
    const rows = [...list.querySelectorAll('li')];
    expect(rows.length).toBeGreaterThan(1);
    expect(text(list)).toContain('Cancelled in the confirmation window');
    expect(text(list)).not.toContain('guard.sh');
    const undos = [...list.querySelectorAll('button')].filter(
      (b) => text(b) === 'Undo',
    );
    // Only changes that were applied, and are not themselves undos, can be undone.
    const applied = fixtureHistory().changes.filter(
      (c) => c.outcome === 'applied' && !c.undoes,
    );
    expect(undos).toHaveLength(applied.length);
    await click(undos[0]);
    expect(changes.calls.some((c) => c.startsWith('undo:'))).toBe(true);
    expect(document.querySelector('.configuration-plan')).not.toBeNull();
  });

  it('lists what was turned off and puts it back, exports it, forgets it or imports another', async () => {
    const changes = await shown();
    await click(button('Turned off'));
    const list = document.querySelector('ul[aria-label="Turned-off hooks"]')!;
    expect(text(list)).toContain('encrypted');
    await click(button('Put back', list));
    expect(changes.calls.some((c) => c.startsWith('plan:restore_hook:'))).toBe(
      true,
    );
    await click(
      button('Cancel', document.querySelector('.configuration-plan')!),
    );
    await click(button('Export…', list));
    expect(changes.calls.some((c) => c.startsWith('export:'))).toBe(true);
    expect(text(document.querySelector('.configuration-feature'))).toContain(
      'Exported',
    );
    await click(button('Import a kept hook…'));
    expect(changes.calls).toContain('import');
    await click(button('Forget', list));
    expect(changes.calls.some((c) => c.startsWith('delete:'))).toBe(true);
  });

  it('says nothing is turned off when nothing is', async () => {
    const changes = new FakeChanges();
    changes.records = [];
    await shown(undefined, changes);
    await click(button('Turned off'));
    expect(text(document.querySelector('.configuration-feature'))).toContain(
      'Nothing is turned off',
    );
  });
});
