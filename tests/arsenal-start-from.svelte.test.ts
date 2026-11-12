import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import ArsenalView from '../src/lib/features/arsenal/ArsenalView.svelte';
import type {
  ArsenalController,
  ArsenalState,
  ArsenalTab,
} from '../src/lib/features/arsenal/controller';
import type { Detail, LibraryItem } from '../src/lib/contracts/arsenal';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const tracer: LibraryItem = {
  id: 'cap-tracer',
  provider: 'codex',
  source: 'codex_home_skills',
  kind: 'skill',
  name: 'tracer',
  description: 'Traces',
  origin: '~/.codex/skills/tracer',
  revision: 1,
  revisions: 1,
  tree_hash: 'a'.repeat(64),
  updated_at: 1,
  compatibility: { status: 'supported', reason: 'own format' },
};

const design: LibraryItem = {
  ...tracer,
  id: 'cap-design',
  provider: 'claude_code',
  source: 'claude_user_skills',
  name: 'design',
  origin: '~/.claude/skills/design',
};

const detailOf = (summary: LibraryItem) =>
  ({
    summary,
    revision: 1,
    compatibility: summary.compatibility,
    dependencies: [],
    files: [{ path: 'SKILL.md', size: 10 }],
    revisions: [
      { number: 1, files: 1, bytes: 10, created_at: 1, current: true },
    ],
  }) as unknown as Detail;

const opened = {
  summary: tracer,
  revision: 1,
  compatibility: tracer.compatibility,
  dependencies: [],
  files: [{ path: 'SKILL.md', size: 10 }],
  revisions: [{ number: 1, files: 1, bytes: 10, created_at: 1, current: true }],
} as unknown as Detail;

function start() {
  const state = $state<ArsenalState>({
    tab: 'library',
    phase: 'ready',
    library: [tracer, design],
    opened,
    file: null,
    discover: null,
    discovering: false,
    chosen: null,
    busy: null,
    bulk: null,
    notice: null,
    presets: [],
    selections: [],
    preview: null,
    chosenDetail: null,
    bindings: [],
    sync: null,
    groups: {},
    movingGroup: null,
    groupFailure: null,
    plan: null,
  });
  const controller = {
    setTab: vi.fn((tab: ArsenalTab) => {
      state.tab = tab;
    }),
    open: vi.fn(),
    showFile: vi.fn(),
    restore: vi.fn(),
    savePreset: vi.fn(async () => true),
    saveSelection: vi.fn(async () => true),
    loadPreview: vi.fn(),
    loadBindings: vi.fn(),
  };
  component = mount(ArsenalView, {
    target: document.body,
    props: {
      visible: true,
      get arsenal() {
        return state;
      },
      controller: controller as unknown as ArsenalController,
    },
  });
  flushSync();
  return { controller, state };
}

const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('button')].find(
    (node) => node.textContent?.trim() === label,
  )!;

const submit = (label: string) => {
  document
    .querySelector<HTMLFormElement>(`form[aria-label="${label}"]`)!
    .dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
  flushSync();
};

describe('Starting from a library capability', () => {
  it('opens a preset named after the capability with it already in, saved without typing', () => {
    const { controller } = start();
    button('Start a preset with this').click();
    flushSync();
    expect(controller.setTab).toHaveBeenCalledWith('presets');
    const name = document.querySelector<HTMLInputElement>(
      'form[aria-label="Preset editor"] input',
    )!;
    expect(name.value).toBe('tracer');
    submit('Preset editor');
    expect(controller.savePreset).toHaveBeenCalledWith({
      id: null,
      name: 'tracer',
      description: '',
      items: [{ capability: 'cap-tracer', revision: null }],
    });
  });

  it('opens a selection for the capability’s own provider with it already in', () => {
    const { controller } = start();
    button('Start a selection with this').click();
    flushSync();
    expect(controller.setTab).toHaveBeenCalledWith('selections');
    submit('Selection editor');
    expect(controller.saveSelection).toHaveBeenCalledWith({
      id: null,
      name: 'tracer',
      provider: 'codex',
      items: [{ capability: 'cap-tracer', revision: null }],
      composites: [],
    });
  });

  it('adds to a preset being edited instead of discarding its unsaved name and items', () => {
    const { controller, state } = start();
    button('Start a preset with this').click();
    flushSync();
    const name = document.querySelector<HTMLInputElement>(
      'form[aria-label="Preset editor"] input',
    )!;
    name.value = 'Frontend kit';
    name.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    state.tab = 'library';
    state.opened = detailOf(design);
    flushSync();
    button('Start a preset with this').click();
    flushSync();
    submit('Preset editor');
    expect(controller.savePreset).toHaveBeenCalledWith({
      id: null,
      name: 'Frontend kit',
      description: '',
      items: [
        { capability: 'cap-tracer', revision: null },
        { capability: 'cap-design', revision: null },
      ],
    });
  });

  it('leaves a selection for another provider untouched and says why', () => {
    const { controller, state } = start();
    button('Start a selection with this').click();
    flushSync();
    state.tab = 'library';
    state.opened = detailOf(design);
    flushSync();
    button('Start a selection with this').click();
    flushSync();
    expect(document.body.textContent).toContain(
      'The selection being edited is for Codex.',
    );
    submit('Selection editor');
    expect(controller.saveSelection).toHaveBeenCalledWith({
      id: null,
      name: 'tracer',
      provider: 'codex',
      items: [{ capability: 'cap-tracer', revision: null }],
      composites: [],
    });
  });

  it('adds a preset to the selection being edited with one button per preset', () => {
    const { controller, state } = start();
    state.library = [
      ...state.library,
      { ...tracer, id: 'cap-notes', name: 'notes' },
    ];
    state.presets = [
      {
        id: 'pre-1',
        name: 'Frontend kit',
        description: '',
        items: [{ capability: 'cap-notes', revision: 1 }],
        created_at: 1,
        updated_at: 1,
      },
    ];
    button('Start a selection with this').click();
    flushSync();
    button('Add Frontend kit').click();
    flushSync();
    submit('Selection editor');
    expect(controller.saveSelection).toHaveBeenCalledWith(
      expect.objectContaining({
        items: [
          { capability: 'cap-tracer', revision: null },
          { capability: 'cap-notes', revision: 1 },
        ],
      }),
    );
  });
});
