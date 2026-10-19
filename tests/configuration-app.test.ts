import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import { listing } from './agent-harness';
import approvals from './fixtures/approval-list-v1.json';
import fixture from './fixtures/configuration-inventory-v1.json';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v3.json';

const fakes = vi.hoisted(() => ({ factory: vi.fn() }));
vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: fakes.factory,
}));

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
  for (let i = 0; i < 30; i++) await Promise.resolve();
  flushSync();
};
const never = () => new Promise(() => undefined);
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const connected = {
  ...views.connected,
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
};

function desktop(inventory: () => unknown = () => fixture) {
  const calls: string[] = [];
  mockIPC((command, payload) => {
    calls.push(command);
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    switch (command) {
      case 'authority_state':
        return request.after === null ? connected : never();
      case 'configuration_inventory':
        return inventory();
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
  return { count: (c: string) => calls.filter((x) => x === c).length };
}
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label))!;
const view = () =>
  document.querySelector<HTMLElement>('.configuration-feature')!;

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
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it('is a view of its own that reads configuration only while it is shown', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  expect(nav('Configuration').disabled).toBe(false);
  expect(view().hidden).toBe(true);
  expect(ipc.count('configuration_inventory')).toBe(0);

  nav('Configuration').click();
  await flush();
  expect(view().hidden).toBe(false);
  expect(document.querySelector('.context-title')?.textContent).toBe(
    'Configuration',
  );
  expect(ipc.count('configuration_inventory')).toBe(1);
  expect(text(view().querySelector('.sessions-count'))).toBe(
    '13 items from 21 places',
  );

  nav('Console').click();
  await flush();
  expect(view().hidden).toBe(true);
});

it('does not take the desktop’s answer on faith', async () => {
  desktop(() => ({
    ...fixture,
    inventory: { ...fixture.inventory, items: 'x' },
  }));
  component = mount(App, { target: document.body });
  await flush();
  nav('Configuration').click();
  await flush();
  expect(text(view())).toContain('The configuration could not be read');
  expect(view().querySelector('.configuration-row')).toBeNull();
});
