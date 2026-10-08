import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import type { WorkspaceState } from '../src/lib/features/terminal/session-manager';
import {
  parseConsoleSessionId,
  parseTerminalId,
} from '../src/lib/contracts/identities';
import { NAV_ITEMS } from '../src/lib/shell/nav';
import { summarizeRuntime } from '../src/lib/shell/runtime';

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
  for (let i = 0; i < 25; i++) await Promise.resolve();
  flushSync();
};
const session = (state: 'starting' | 'running' | 'failed' | 'exited') => ({
  id: parseConsoleSessionId('00000000-0000-4000-8000-000000000001'),
  title: 'Terminal 1',
  view: { state },
});
const workspace = (over: Partial<WorkspaceState>): WorkspaceState => ({
  sessions: [],
  creating: false,
  ...over,
});

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
  mockIPC((command) => {
    if (command === 'create_terminal') return info;
    if (command === 'terminal_read_output') return new Promise(() => undefined);
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return info;
  });
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  await flush();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it('maps the terminal workspace to one honest runtime status', () => {
  expect(summarizeRuntime(workspace({}))).toEqual({
    label: 'No terminals',
    tone: 'idle',
  });
  expect(summarizeRuntime(workspace({ creating: true }))).toEqual({
    label: 'Starting',
    tone: 'pending',
  });
  expect(
    summarizeRuntime(
      workspace({
        sessions: [session('running')],
        active: session('running').id,
      }),
    ),
  ).toEqual({ label: 'Running', tone: 'ok' });
  expect(
    summarizeRuntime(
      workspace({
        sessions: [session('failed')],
        active: session('failed').id,
      }),
    ),
  ).toEqual({ label: 'Failed', tone: 'error' });
  expect(
    summarizeRuntime(
      workspace({
        sessions: [session('exited')],
        active: session('exited').id,
      }),
    ),
  ).toEqual({ label: 'Exited', tone: 'warn' });
  expect(summarizeRuntime(workspace({ error: 'spawn_failed' }))).toEqual({
    label: 'Error',
    tone: 'error',
  });
});

it('lists every planned view but enables only Console', () => {
  expect(NAV_ITEMS.map((item) => item.label)).toEqual([
    'Console',
    'Sessions',
    'Agents',
    'Approvals',
    'Events',
    'Settings',
  ]);
  expect(NAV_ITEMS.filter((item) => item.available).map((i) => i.id)).toEqual([
    'console',
  ]);
});

it('renders the shell with brand, navigation placeholders and Console selected', async () => {
  component = mount(App, { target: document.body });
  await flush();
  expect(document.querySelector('h1')?.textContent).toBe('Wyss AI');
  const nav = Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  );
  expect(nav).toHaveLength(6);
  expect(nav[0].getAttribute('aria-current')).toBe('page');
  expect(nav.slice(1).every((b) => b.disabled)).toBe(true);
  expect(document.querySelector('.context-title')?.textContent).toBe('Console');
  expect(document.querySelector('header')?.textContent).toContain('Runtime');
  // The workspace opens one terminal on mount, so the pill reports it running.
  expect(document.querySelector('.pill[data-tone]')?.textContent).toContain(
    'Running',
  );
});

it('reports a failed terminal in the top-bar runtime status', async () => {
  mockIPC((command) => {
    if (command === 'create_terminal')
      return Promise.reject({ code: 'spawn_failed', diagnostic: 'private' });
    return info;
  });
  component = mount(App, { target: document.body });
  await flush();
  const pill = document.querySelector('.pill[data-tone]');
  expect(pill?.getAttribute('data-tone')).toBe('error');
  expect(pill?.textContent).toContain('Failed');
  expect(document.querySelector('header')?.textContent).not.toContain(
    'private',
  );
});
