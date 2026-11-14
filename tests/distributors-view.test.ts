import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import { listing } from './agent-harness';
import { launcher, profiles } from './native-harness';
import approvals from './fixtures/approval-list-v1.json';
import inventory from './fixtures/configuration-inventory-v1.json';
import catalogues from './fixtures/model-catalogue-v1.json';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v4.json';

const fakes = vi.hoisted(() => ({ factory: vi.fn() }));
vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: fakes.factory,
}));

const KEY = 'wyss-console.distributors-seen';
let component: ReturnType<typeof mount> | undefined;
const info = {
  terminal_id: parseTerminalId('55555555-5555-4555-8555-555555555555'),
  shell: '/bin/fish',
  cwd: '/example',
  observed_pid: 123,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};
const flush = async () => {
  for (let i = 0; i < 40; i++) await Promise.resolve();
  flushSync();
};
const never = () => new Promise(() => undefined);
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const catalogueWith = (...ids: string[]) => ({
  ...catalogues.listed,
  entries: [
    ...catalogues.listed.entries,
    ...ids.map((model_id) => ({ ...catalogues.listed.entries[0], model_id })),
  ],
});

/** A desktop whose answers the test changes between refreshes. */
function desktop() {
  const calls: string[] = [];
  const answers = {
    /** A value, or a function called on each read (a promise leaves the read in flight). */
    catalogue: catalogues.listed as unknown,
    inventory: (() => inventory) as () => unknown,
    launcherFails: false,
  };
  mockIPC((command, payload) => {
    calls.push(command);
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    switch (command) {
      case 'authority_state':
        return request.after === null ? views.connected : never();
      case 'native_launcher':
        if (answers.launcherFails) throw { code: 'internal', reasons: [] };
        return launcher();
      case 'native_readiness':
        return { m9_native_status: 'ready' };
      case 'native_sessions':
        return [];
      case 'native_profiles':
        return profiles();
      case 'managed_model_catalogue':
        return typeof answers.catalogue === 'function'
          ? (answers.catalogue as () => unknown)()
          : answers.catalogue;
      case 'configuration_inventory':
        return answers.inventory();
      case 'configuration_changes':
        return { result: 'same', revision: 1 };
      case 'managed_sessions_page':
        return pages.loaded;
      case 'managed_turns':
        return listing([]);
      case 'managed_approvals':
        return approvals.empty;
      case 'terminal_read_output':
        return never();
      case 'get_app_info':
        return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
      default:
        return info;
    }
  });
  return { answers, count: (c: string) => calls.filter((x) => x === c).length };
}
const rail = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.rail-list .nav-item'),
  ).find((b) => b.getAttribute('title') === label)!;
const view = () =>
  document.querySelector<HTMLElement>('.distributors-feature')!;
const card = (id: string) =>
  view().querySelector<HTMLElement>(`.dist-card[data-id="${id}"]`)!;
const status = (id: string) => text(card(id).querySelector('.dist-status'));
const rows = (id: string) =>
  Array.from(card(id).querySelectorAll('.dist-models li')).map(text);
const refresh = async () => {
  const button = Array.from(view().querySelectorAll('button')).find(
    (b) => text(b) === 'Refresh models',
  )!;
  expect(button.disabled).toBe(false);
  button.click();
  await flush();
};

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
  fakes.factory.mockImplementation((): TerminalRenderer => ({
    fit: () => info.size,
    write: async () => true,
    input: () => () => undefined,
    setInput: vi.fn(),
    setActive: vi.fn(),
    focus: vi.fn(),
    dispose: vi.fn(),
  }));
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.innerHTML = '';
  localStorage.clear();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it('opens from the rail, reads only while shown, and draws what the desktop answered', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  expect(view().hidden).toBe(true);
  expect(view().inert).toBe(true);
  // The tools are discovered once when the window starts, whichever view is shown.
  const before = ipc.count('native_launcher');
  expect(before).toBe(1);
  expect(ipc.count('configuration_inventory')).toBe(0);
  expect(ipc.count('managed_model_catalogue')).toBe(0);

  rail('Distributors').click();
  await flush();
  expect(view().hidden).toBe(false);
  expect(rail('Distributors').getAttribute('aria-current')).toBe('page');
  // Providers is a view of its own, under More: the rail no longer opens it.
  expect(
    document.querySelector<HTMLElement>('.providers-feature')!.hidden,
  ).toBe(true);
  // The cards say what the view is: no title is drawn over them.
  expect(document.querySelector('.context-title')).toBeNull();
  // Showing the view discovers nothing again: only Refresh models does.
  expect(ipc.count('native_launcher')).toBe(before);
  expect(ipc.count('configuration_inventory')).toBe(1);
  expect(ipc.count('managed_model_catalogue')).toBe(1);

  expect(
    Array.from(view().querySelectorAll('.dist-card h3')).map(text),
  ).toEqual(['Anthropic', 'OpenAI', 'Kimi', 'OpenCode']);
  expect(Array.from(view().querySelectorAll('.dist-tool')).map(text)).toEqual([
    'Claude Code',
    'Codex',
    'Kimi CLI',
    'OpenCode',
  ]);
  expect(status('anthropic')).toBe('Installed · 2.1.303 (Claude Code)');
  expect(card('anthropic').querySelector('.dist-status.ok')).not.toBeNull();
  expect(status('openai')).toBe('Tool not installed');
  expect(card('openai').querySelector('.dist-status.ok')).toBeNull();
  expect(status('kimi')).toBe('Not configured');
  expect(status('opencode')).toBe('Unknown');
  expect(text(view())).not.toMatch(/connected/i);

  expect(rows('anthropic')).toEqual([
    'Configured model: opus',
    'Profile Mine: sonnet',
  ]);
  expect(text(card('anthropic'))).toContain(
    'This tool does not list its models to the Console.',
  );
  // The first read is kept and nothing in it is new.
  expect(rows('openai')).toEqual(['model-a', 'model-b']);
  expect(view().querySelector('.dist-new')).toBeNull();
  expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual([
    'model-a',
    'model-b',
  ]);
  expect(rows('kimi')).toEqual([]);
  expect(text(view().querySelector('.dist-read'))).toMatch(/^Last read: \S+/);
  expect(text(view())).toContain('Only Codex reports new models.');
  // No way to add a distributor, and no credential is shown.
  expect(Array.from(view().querySelectorAll('button')).map(text)).toEqual([
    'Refresh models',
  ]);
});

it('reads the three sources again on Refresh models and marks only what Codex newly lists', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  rail('Distributors').click();
  await flush();

  const before = ipc.count('native_launcher');
  ipc.answers.catalogue = catalogueWith('model-c');
  await refresh();
  expect(ipc.count('native_launcher')).toBe(before + 1);
  expect(ipc.count('configuration_inventory')).toBe(2);
  expect(ipc.count('managed_model_catalogue')).toBe(2);
  expect(rows('openai')).toEqual(['model-a', 'model-b', 'model-c new model']);
  expect(view().querySelectorAll('.dist-new')).toHaveLength(1);
  // Nothing of the other tools is ever marked.
  expect(card('anthropic').querySelector('.dist-new')).toBeNull();

  // The mark is gone at the next refresh.
  await refresh();
  expect(ipc.count('managed_model_catalogue')).toBe(3);
  expect(rows('openai')).toEqual(['model-a', 'model-b', 'model-c']);
  expect(view().querySelector('.dist-new')).toBeNull();

  // A runtime that is not ready shows why, marks nothing and leaves what was seen.
  const kept = localStorage.getItem(KEY);
  ipc.answers.catalogue = catalogues.not_ready;
  await refresh();
  expect(rows('openai')).toEqual([]);
  expect(text(card('openai'))).toContain(
    'The managed Codex runtime is not running, so there is no list.',
  );
  expect(localStorage.getItem(KEY)).toBe(kept);
  ipc.answers.catalogue = catalogueWith('model-c', 'model-d');
  await refresh();
  expect(rows('openai')).toEqual([
    'model-a',
    'model-b',
    'model-c',
    'model-d new model',
  ]);
});

it('says unknown, not missing, when the tools did not answer a refresh', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  rail('Distributors').click();
  await flush();
  expect(status('anthropic')).toBe('Installed · 2.1.303 (Claude Code)');

  ipc.answers.launcherFails = true;
  await refresh();
  expect(status('anthropic')).toBe('Unknown');
  expect(status('openai')).toBe('Unknown');
  expect(status('opencode')).toBe('Unknown');
  expect(status('kimi')).toBe('Not configured');

  // It answers again: what it says is shown again.
  ipc.answers.launcherFails = false;
  await refresh();
  expect(status('anthropic')).toBe('Installed · 2.1.303 (Claude Code)');
  expect(status('openai')).toBe('Tool not installed');
});

const refreshButton = () =>
  Array.from(view().querySelectorAll('button')).find(
    (b) => text(b) === 'Refresh models',
  )!;
const more = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.rail-menu .nav-item'),
  ).find((b) => text(b) === label)!;
/** The fixture inventory with Claude Code's `model` set to `model`. */
const inventoryWith = (model: string) => {
  const copy = structuredClone(inventory);
  for (const item of copy.inventory.items)
    if (
      item.provider === 'claude_code' &&
      item.kind === 'model_preference' &&
      item.name === 'model' &&
      item.detail?.kind === 'text'
    )
      (item.detail.value as { text: string }).text = model;
  return copy;
};

it('waits for a Codex read already in flight before it compares, so its answer is the one marked', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  rail('Distributors').click();
  await flush();
  expect(rows('openai')).toEqual(['model-a', 'model-b']);

  // The Models view starts a read of the list, and it does not answer yet.
  let answer!: (value: unknown) => void;
  ipc.answers.catalogue = () =>
    new Promise((resolve) => {
      answer = resolve;
    });
  more('Models').click();
  await flush();
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.models-view button'),
  )
    .find((b) => text(b) === 'Read again')!
    .click();
  await flush();
  const reads = ipc.count('managed_model_catalogue');

  // Refresh models while it is in flight: no second read, and nothing is compared yet.
  rail('Distributors').click();
  await flush();
  refreshButton().click();
  await flush();
  expect(ipc.count('managed_model_catalogue')).toBe(reads);
  expect(refreshButton().disabled).toBe(true);
  expect(refreshButton().getAttribute('aria-busy')).toBe('true');
  expect(rows('openai')).toEqual(['model-a', 'model-b']);

  // It answers with a model not seen before: that answer is what is compared.
  answer(catalogueWith('model-c'));
  await flush();
  expect(refreshButton().disabled).toBe(false);
  expect(rows('openai')).toEqual(['model-a', 'model-b', 'model-c new model']);
  expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual([
    'model-a',
    'model-b',
    'model-c',
  ]);
});

it('waits for the configuration read, and says when the configured model shown is from an earlier one', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  rail('Distributors').click();
  await flush();
  expect(rows('anthropic')[0]).toBe('Configured model: opus');
  const note = 'Configured models are from an earlier read.';
  expect(text(view())).not.toContain(note);

  // The refresh is not over until the agents' files were read again.
  let answer!: (value: unknown) => void;
  ipc.answers.inventory = () =>
    new Promise((resolve) => {
      answer = resolve;
    });
  refreshButton().click();
  await flush();
  expect(refreshButton().getAttribute('aria-busy')).toBe('true');
  expect(refreshButton().disabled).toBe(true);
  expect(rows('anthropic')[0]).toBe('Configured model: opus');
  answer(inventoryWith('haiku'));
  await flush();
  expect(refreshButton().disabled).toBe(false);
  expect(rows('anthropic')[0]).toBe('Configured model: haiku');
  expect(text(view())).not.toContain(note);

  // The read fails: what is shown is the earlier read, and the card says so.
  ipc.answers.inventory = () => {
    throw { code: 'internal' };
  };
  await refresh();
  expect(rows('anthropic')[0]).toBe('Configured model: haiku');
  expect(text(card('anthropic'))).toContain(note);
  // A card that shows no configured model claims nothing about one.
  expect(text(card('opencode'))).not.toContain(note);
  expect(text(card('openai'))).not.toContain(note);

  // It is read whole again: nothing is said to be older.
  ipc.answers.inventory = () => inventoryWith('sonnet');
  await refresh();
  expect(rows('anthropic')[0]).toBe('Configured model: sonnet');
  expect(text(view())).not.toContain(note);
});
