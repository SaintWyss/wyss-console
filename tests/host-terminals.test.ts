import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalSessionManager,
  type WorkspaceState,
} from '../src/lib/features/terminal/session-manager';
import { TerminalController } from '../src/lib/features/terminal/controller';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseHostTerminals } from '../src/lib/contracts/terminal';

vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: vi.fn(),
}));

const flush = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
};
const terminal = (n: number, state = 'running') => ({
  terminal_id: `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`,
  shell: '/bin/fish',
  cwd: '/home/u',
  observed_pid: 100 + n,
  size: { rows: 24, cols: 80 },
  state,
  exit: null,
  error: null,
});
const held = [
  { terminal: terminal(1), title: 'Terminal 1', created_at: 10, native: null },
  {
    terminal: terminal(2),
    title: 'Claude Code · app',
    created_at: 20,
    native: {
      session_id: 'a'.repeat(32),
      agent_name: 'Claude Code',
      project: '/home/u/app',
      profile: 'Quiet',
      through_shell: true,
    },
  },
];

let state: WorkspaceState;
let calls: { command: string; request: Record<string, unknown> }[];
let list: unknown;
let manager: TerminalSessionManager;

const renderer = (): TerminalRenderer => ({
  fit: vi.fn(() => ({ rows: 24, cols: 80 })),
  write: vi.fn(async () => true),
  input: vi.fn(() => vi.fn()),
  setInput: vi.fn(),
  setActive: vi.fn(),
  focus: vi.fn(),
  dispose: vi.fn(),
});
function attachAll() {
  for (const s of state.sessions)
    manager.attach(s.id, document.createElement('div'));
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
  calls = [];
  list = held;
  state = { sessions: [], tabs: [], creating: false };
  mockIPC((command, args) => {
    const request =
      (args as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'host_terminals') return list;
    if (command === 'terminal_read_output') return new Promise(() => undefined);
    if (command === 'create_terminal') return terminal(9);
    if (command === 'terminal_redraw') return null;
    return terminal(1);
  });
  manager = new TerminalSessionManager(
    (value) => {
      state = value;
    },
    (host, update, adopted, title) =>
      new TerminalController(host, renderer(), update, adopted, title),
    true,
  );
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const count = (c: string) => calls.filter((x) => x.command === c);

it('shows again what the host holds when a window opens, with titles and agent tags, and creates nothing', async () => {
  manager.start();
  await flush();
  attachAll();
  await flush();
  expect(count('create_terminal')).toHaveLength(0);
  expect(state.sessions.map((s) => s.title)).toEqual([
    'Terminal 1',
    'Claude Code · app',
  ]);
  expect(state.sessions[1]!.native).toMatchObject({
    sessionId: 'a'.repeat(32),
    agent: 'Claude Code',
    profile: 'Quiet',
  });
  expect(state.sessions[0]!.native).toBeUndefined();
  expect(state.active).toBe(state.sessions[0]!.id);
  // Each reads the terminal it was given.
  expect(
    count('terminal_read_output').map((c) => c.request.terminal_id),
  ).toEqual(
    expect.arrayContaining([terminal(1).terminal_id, terminal(2).terminal_id]),
  );
});

it('asks each restored program to draw its screen again, once attached', async () => {
  manager.start();
  await flush();
  attachAll();
  await flush();
  expect(
    count('terminal_redraw')
      .map((c) => c.request.terminal_id)
      .sort(),
  ).toEqual([terminal(1).terminal_id, terminal(2).terminal_id].sort());
});

it('makes one terminal, named, when the host holds none', async () => {
  list = [];
  manager.start();
  await flush();
  attachAll();
  await flush();
  expect(count('create_terminal')).toHaveLength(1);
  expect(count('create_terminal')[0]!.request).toMatchObject({
    title: 'Terminal 1',
  });
});

it('leaving the window lets the terminals go and does not close them', async () => {
  manager.start();
  await flush();
  attachAll();
  await flush();
  await manager.dispose();
  expect(count('close_terminal')).toHaveLength(0);
});

it('closing one tab does close that terminal in the host', async () => {
  manager.start();
  await flush();
  attachAll();
  await flush();
  await manager.close(state.sessions[1]!.id);
  expect(count('close_terminal').map((c) => c.request.terminal_id)).toEqual([
    terminal(2).terminal_id,
  ]);
});

it('takes anything that is not a list of terminals as no host', () => {
  expect(parseHostTerminals({ terminal_id: 'x' })).toEqual([]);
  expect(parseHostTerminals([{ title: 1 }, null, 'x'])).toEqual([]);
  expect(parseHostTerminals(held)).toHaveLength(2);
});
