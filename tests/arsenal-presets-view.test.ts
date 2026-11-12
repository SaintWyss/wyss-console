import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import PresetsTab from '../src/lib/features/arsenal/PresetsTab.svelte';
import type {
  ArsenalController,
  ArsenalState,
} from '../src/lib/features/arsenal/controller';
import { planKey } from '../src/lib/features/arsenal/presets';
import type {
  ArsenalProvider,
  LibraryItem,
  Preview,
  Selection,
} from '../src/lib/contracts/arsenal';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';

function item(name: string, overrides: Partial<LibraryItem> = {}): LibraryItem {
  return {
    id: `cap-${name}`,
    provider: 'claude_code',
    source: 'claude_user_skills',
    kind: 'skill',
    name,
    description: `Does ${name}. More about it.`,
    origin: `~/.claude/skills/${name}`,
    revision: 1,
    revisions: 1,
    tree_hash: 'a'.repeat(64),
    updated_at: 1,
    compatibility: { status: 'supported', reason: 'own format' },
    ...overrides,
  };
}

const frontend = item('frontend-design');
const image = item('image-to-code');

const selection = (overrides: Partial<Selection> = {}): Selection => ({
  id: 'sel-1',
  name: 'Estudio',
  provider: 'claude_code',
  items: [{ capability: 'cap-frontend-design', revision: null }],
  composites: [],
  created_at: 1,
  updated_at: 1,
  ...overrides,
});

const previewOf = (states: Preview['items'][number]['state'][]): Preview => ({
  selection: selection({ id: '', name: '' }),
  status: 'not_applied',
  items: states.map((state, index) => ({
    capability: `cap-${index}`,
    state,
    kind: 'skill',
    name: `n${index}`,
    provider: 'claude_code',
    revision: null,
    pinned: false,
    tree_hash: null,
    compatibility: 'supported',
    reason: '',
    dependencies: [],
  })),
  conflicts: [],
  dependencies: 'analysed',
});

function controllerMock() {
  return {
    checkDraft: vi.fn(async () => undefined),
    saveSelection: vi.fn(async (input: Record<string, unknown>) => ({
      ...selection(),
      id: 'sel-new',
      name: input.name,
      items: input.items,
      composites: input.composites,
    })),
    deleteSelection: vi.fn(async () => true),
    moveGroup: vi.fn(async () => undefined),
    loadSelections: vi.fn(async () => undefined),
  };
}
type Mocked = ReturnType<typeof controllerMock>;

function stateWith(overrides: Partial<ArsenalState> = {}): ArsenalState {
  return {
    tab: 'presets',
    phase: 'ready',
    library: [frontend, image],
    opened: null,
    file: null,
    discover: null,
    discovering: false,
    chosen: null,
    chosenDetail: null,
    busy: null,
    bulk: null,
    notice: null,
    presets: [],
    selections: [],
    preview: null,
    bindings: [],
    sync: null,
    groups: {},
    movingGroup: null,
    groupFailure: null,
    plan: null,
    selectionsFailure: null,
    ...overrides,
  };
}

function render(
  state: ArsenalState,
  mock: Mocked = controllerMock(),
  provider: ArsenalProvider = 'claude_code',
) {
  component = mount(PresetsTab, {
    target: document.body,
    props: {
      provider,
      arsenal: state,
      controller: mock as unknown as ArsenalController,
    },
  });
  flushSync();
  return { root: document.body, controller: mock };
}

/** The button whose text is exactly this (spaces collapsed). */
function button(label: string): HTMLButtonElement {
  const found = Array.from(document.querySelectorAll('button')).find(
    (node) => text(node) === label,
  );
  if (!found) throw new Error(`no button "${label}"`);
  return found;
}

function groupToggle(label: string): HTMLButtonElement {
  const found = Array.from(
    document.querySelectorAll<HTMLButtonElement>('.group-toggle'),
  ).find((node) => text(node).includes(label));
  if (!found) throw new Error(`no group "${label}"`);
  return found;
}

function checkbox(name: string): HTMLInputElement {
  const found = document.querySelector<HTMLInputElement>(
    `input[aria-label="${name}"]`,
  );
  if (!found) throw new Error(`no checkbox for ${name}`);
  return found;
}

function tick(input: HTMLInputElement, value: boolean) {
  input.checked = value;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}

function type(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  flushSync();
}

describe('Presets screen', () => {
  it('says there are no presets for this provider and offers a new one', () => {
    render(stateWith());
    expect(text(document.querySelector('.presets-list'))).toContain(
      'No presets for Claude Code yet.',
    );
    expect(button('+ New preset')).toBeTruthy();
  });

  it('says nothing is chosen, shows no totals, and checks no draft with nothing in it', () => {
    const { controller, root } = render(stateWith());
    expect(text(root.querySelector('.totals'))).toBe(
      'Nothing chosen yet. Tick a group or a capability.',
    );
    expect(controller.checkDraft).toHaveBeenLastCalledWith(
      'claude_code',
      [],
      [],
    );
  });

  it('starts on Rules when the provider has rules, and on All when it has none', () => {
    const pressed = (root: Element) =>
      [...root.querySelectorAll('button[aria-pressed="true"]')]
        .map((node) => text(node))
        .filter((label) =>
          ['All', 'Skills', 'Agents', 'Rules'].includes(label),
        );
    const noRules = render(stateWith());
    flushSync();
    expect(pressed(noRules.root)).not.toContain('Rules');
    const rule = item('invariantes', { kind: 'rule' });
    const withRules = render(stateWith({ library: [frontend, rule] }));
    flushSync();
    expect(pressed(withRules.root)).toContain('Rules');
    expect(withRules.root.textContent).toContain('invariantes');
    expect(withRules.root.textContent).not.toContain('frontend-design');
  });

  it('fills the group meter when a capability is ticked, checks the draft, and never saves it', () => {
    const { controller, root } = render(stateWith());
    flushSync();
    controller.checkDraft.mockClear();
    groupToggle('Design and interface').click();
    flushSync();
    tick(checkbox('frontend-design'), true);
    expect(root.querySelectorAll('.cell[data-filled="true"]')).toHaveLength(1);
    expect(text(root.querySelector('.meter-row'))).toContain(
      'Design and interface',
    );
    expect(controller.checkDraft).toHaveBeenLastCalledWith(
      'claude_code',
      [{ capability: 'cap-frontend-design', revision: null }],
      [],
    );
    expect(controller.saveSelection).not.toHaveBeenCalled();
  });

  it('keeps Save preset disabled and asks for a name until the preset has one', async () => {
    const { controller } = render(stateWith());
    groupToggle('Design and interface').click();
    flushSync();
    tick(checkbox('frontend-design'), true);
    expect(button('Save preset').disabled).toBe(true);
    expect(text(document.querySelector('.field .help'))).toBe(
      'Name the preset',
    );
    type(
      document.querySelector<HTMLInputElement>('input[type="text"]')!,
      'Estudio',
    );
    expect(button('Save preset').disabled).toBe(false);
    button('Save preset').click();
    await Promise.resolve();
    expect(controller.saveSelection).toHaveBeenCalledWith({
      id: null,
      name: 'Estudio',
      provider: 'claude_code',
      items: [{ capability: 'cap-frontend-design', revision: null }],
      composites: [],
    });
  });

  it('says the draft could not be checked, and Try again asks the desktop again', () => {
    const items = [{ capability: 'cap-frontend-design', revision: null }];
    const { controller, root } = render(
      stateWith({
        selections: [selection()],
        plan: {
          key: planKey('claude_code', items, []),
          preview: null,
          failed: true,
        },
      }),
    );
    const row = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.preset-row'),
    ).find((node) => text(node).includes('Estudio'))!;
    row.click();
    flushSync();
    expect(text(root.querySelector('.totals'))).toContain(
      'Could not be checked.',
    );
    button('Try again').click();
    expect(controller.checkDraft).toHaveBeenLastCalledWith(
      'claude_code',
      items,
      [],
      true,
    );
  });

  it('shows the totals and how the terminal starts once the desktop answered', () => {
    const items = [{ capability: 'cap-frontend-design', revision: null }];
    const { root } = render(
      stateWith({
        selections: [selection()],
        plan: {
          key: planKey('claude_code', items, []),
          preview: previewOf(['resolved']),
          failed: false,
        },
      }),
    );
    Array.from(
      document.querySelectorAll<HTMLButtonElement>('.preset-row'),
    )[0]!.click();
    flushSync();
    expect(text(root.querySelector('.totals'))).toBe('1 on · 1 off');
    expect(text(root.querySelector('.starts'))).toBe(
      'Claude Code starts with 1 capabilities from this preset. The other 1 installed skills are switched off in that terminal.',
    );
  });

  it('says a refused group move changed nothing, on the row of that capability', () => {
    render(
      stateWith({
        groupFailure: {
          identity: 'cap-frontend-design',
          text: 'Could not be saved. Nothing changed.',
        },
      }),
    );
    type(
      document.querySelector<HTMLInputElement>('input[type="search"]')!,
      'frontend',
    );
    expect(text(document.querySelector('.capability .error'))).toBe(
      'Could not be saved. Nothing changed.',
    );
  });

  it('moves a capability to another group through Move to group', () => {
    const { controller } = render(stateWith());
    type(
      document.querySelector<HTMLInputElement>('input[type="search"]')!,
      'frontend',
    );
    const row = document.querySelector<HTMLElement>('.capability')!;
    Array.from(row.querySelectorAll<HTMLButtonElement>('button'))
      .find((node) => text(node) === 'Move to group…')!
      .click();
    flushSync();
    Array.from(
      row.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]'),
    )
      .find((node) => text(node) === 'Study and knowledge')!
      .click();
    expect(controller.moveGroup).toHaveBeenCalledWith(
      'cap-frontend-design',
      'study',
    );
  });

  it('does not let a capability the provider cannot use be ticked, and says why', () => {
    const blocked = item('legacy-skill', {
      compatibility: {
        status: 'unsupported',
        reason: 'Its format is not read here',
      },
    });
    render(stateWith({ library: [blocked] }));
    type(
      document.querySelector<HTMLInputElement>('input[type="search"]')!,
      'legacy',
    );
    expect(checkbox('legacy-skill').disabled).toBe(true);
    expect(text(document.querySelector('.capability-why'))).toBe(
      'Its format is not read here',
    );
  });

  it('asks before leaving a preset with unsaved changes, and keeps the draft when asked to keep editing', () => {
    const other = selection({ id: 'sel-2', name: 'Diseño', items: [] });
    const { root } = render(stateWith({ selections: [selection(), other] }));
    const rowOf = (name: string) =>
      Array.from(root.querySelectorAll<HTMLButtonElement>('.preset-row')).find(
        (node) => text(node).includes(name),
      )!;
    rowOf('Estudio').click();
    flushSync();
    groupToggle('Design and interface').click();
    flushSync();
    tick(checkbox('frontend-design'), false);
    rowOf('Diseño').click();
    flushSync();
    expect(button('Discard changes')).toBeTruthy();
    expect(text(root.querySelector('.notice'))).toContain('Unsaved changes');
    button('Keep editing').click();
    flushSync();
    expect(root.querySelector('.notice')).toBeNull();
    expect(rowOf('Estudio').getAttribute('aria-current')).toBe('true');
  });

  it('deletes a preset only after the owner confirms in line', async () => {
    const { controller, root } = render(
      stateWith({ selections: [selection()] }),
    );
    Array.from(
      root.querySelectorAll<HTMLButtonElement>('.preset-row'),
    )[0]!.click();
    flushSync();
    button('Delete').click();
    flushSync();
    expect(text(root.querySelector('.preset-actions'))).toContain(
      'Delete Estudio?',
    );
    expect(controller.deleteSelection).not.toHaveBeenCalled();
    button('Delete').click();
    await Promise.resolve();
    expect(controller.deleteSelection).toHaveBeenCalledWith('sel-1');
  });

  it('says the presets could not be read in the list, and Try again reads them again', () => {
    const { controller, root } = render(
      stateWith({
        selectionsFailure:
          'The Arsenal library could not be opened. Check that the Wyss data folder is writable, then Refresh.',
      }),
    );
    const list = root.querySelector('.presets-list');
    expect(text(list?.querySelector('.notice'))).toContain(
      'The Arsenal library could not be opened.',
    );
    expect(text(list)).not.toContain('No presets for Claude Code yet.');
    button('Try again').click();
    expect(controller.loadSelections).toHaveBeenCalledTimes(1);
  });
});
