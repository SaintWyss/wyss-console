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
import views from './fixtures/authority-view-v1.json';

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
const never = () => new Promise(() => undefined);
const pill = (name: 'terminal' | 'agent' | 'authority') =>
  document.querySelector(`.pill.${name}`);
/** Terminal IPC that works, with the authority projection answered by `authority`. */
function ipc(
  authority: (after: number | null) => unknown,
  terminal: (command: string) => unknown = () => info,
) {
  const commands: string[] = [];
  mockIPC((command, payload) => {
    commands.push(command);
    if (command === 'authority_state')
      return authority(
        (payload as { request: { after: number | null } }).request.after,
      );
    if (command === 'terminal_read_output') return never();
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return terminal(command);
  });
  return commands;
}
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
  ipc(never);
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

it('lists every planned view and enables the ones that exist', () => {
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
    'sessions',
    'agents',
    'approvals',
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
  expect(nav.slice(0, 4).every((b) => !b.disabled)).toBe(true);
  // Events and Settings do not exist yet.
  expect(nav.slice(4).every((b) => b.disabled)).toBe(true);
  // Nothing waits for a decision: the Approvals item carries no count.
  expect(document.querySelector('.nav-tag.waiting')).toBeNull();
  expect(document.querySelector('.context-title')?.textContent).toBe('Console');
  // The workspace opens one terminal on mount, so the pill reports it running.
  expect(pill('terminal')?.textContent).toContain('Terminal');
  expect(pill('terminal')?.textContent).toContain('Running');
  // Nothing was read from the authority yet, and nothing is claimed about it.
  expect(pill('authority')?.textContent).toContain('Connecting');
  expect(pill('agent')?.textContent).toContain('Unknown');
});

it('reports a failed terminal in the top-bar runtime status', async () => {
  ipc(
    (after) => (after === null ? views.connected : never()),
    (command) =>
      command === 'create_terminal'
        ? Promise.reject({ code: 'spawn_failed', diagnostic: 'private' })
        : info,
  );
  component = mount(App, { target: document.body });
  await flush();
  expect(pill('terminal')?.getAttribute('data-tone')).toBe('error');
  expect(pill('terminal')?.textContent).toContain('Failed');
  expect(document.querySelector('header')?.textContent).not.toContain(
    'private',
  );
  // A terminal that failed says nothing about the authority.
  expect(pill('authority')?.textContent).toContain('Connected');
});

it('shows the authority, its sessions and the managed agent from the projection', async () => {
  ipc((after) => (after === null ? views.connected : never()));
  component = mount(App, { target: document.body });
  await flush();
  const authority = pill('authority');
  expect(authority?.getAttribute('data-tone')).toBe('ok');
  expect(authority?.textContent).toContain('Connected');
  expect(authority?.querySelector('.pill-note')?.textContent).toBe('1 session');
  expect(authority?.getAttribute('title')).toContain('1 prepared, 0 closed');
  const agent = pill('agent');
  expect(agent?.textContent).toContain('Codex');
  expect(agent?.textContent).toContain('Failed');
  expect(agent?.getAttribute('data-tone')).toBe('error');
  // The projection is all the window gets: the status area renders no identity.
  expect(document.querySelector('.status-cluster')?.innerHTML).not.toMatch(
    /[0-9a-f]{8}-[0-9a-f]{4}-/i,
  );
});

it('keeps the human terminal working whatever the authority does', async () => {
  for (const [authority, label, tone] of [
    [() => views.waiting_absent, 'Offline', 'idle'],
    [() => Promise.reject({ code: 'busy' }), 'Unavailable', 'error'],
    [() => ({ method: 'runtime/start' }), 'Unavailable', 'error'],
    [never, 'Connecting', 'pending'],
  ] as const) {
    const commands = ipc((after) => (after === null ? authority() : never()));
    component = mount(App, { target: document.body });
    await flush();
    expect(pill('authority')?.textContent, label).toContain(label);
    expect(pill('authority')?.getAttribute('data-tone')).toBe(tone);
    expect(pill('agent')?.textContent).toContain('Unknown');
    // The terminal was created, is running and has its tab.
    expect(pill('terminal')?.textContent).toContain('Running');
    expect(pill('terminal')?.getAttribute('data-tone')).toBe('ok');
    expect(document.querySelectorAll('[role="tab"]')).toHaveLength(1);
    expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(1);
    await unmount(component);
    component = undefined;
    document.body.replaceChildren();
  }
});

it('follows authority restarts without touching the terminal workspace', async () => {
  const answers: ((view: unknown) => void)[] = [];
  const commands = ipc(() => new Promise((resolve) => answers.push(resolve)));
  component = mount(App, { target: document.body });
  await flush();
  const lost = { ...views.waiting_absent, revision: 7, reason: 'lost' };
  const back = {
    ...views.connected,
    revision: 9,
    authority: { ...views.connected.authority, restarts: 1 },
    agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
  };
  for (const [view, authority, agent] of [
    [views.waiting_absent, 'Offline', 'Unknown'],
    [views.connected, 'Connected', 'Failed'],
    [lost, 'Reconnecting', 'Unknown'],
    [back, 'Connected', 'Ready'],
  ] as const) {
    answers.shift()!(view);
    await flush();
    expect(pill('authority')?.textContent).toContain(authority);
    expect(pill('agent')?.textContent).toContain(agent);
    expect(pill('terminal')?.textContent).toContain('Running');
  }
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(1);
  expect(commands.filter((c) => c === 'close_terminal')).toHaveLength(0);
  expect(document.querySelectorAll('[role="tab"]')).toHaveLength(1);
});
