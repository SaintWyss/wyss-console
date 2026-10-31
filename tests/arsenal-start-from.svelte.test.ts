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
    library: [tracer],
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
  return controller;
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
    const controller = start();
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
    const controller = start();
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
});
