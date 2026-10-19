import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalSessionManager,
  type WorkspaceState,
} from '../src/lib/features/terminal/session-manager';
import { TerminalController } from '../src/lib/features/terminal/controller';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { adoptTerminal } from '../src/lib/desktop/terminal-client';
import { reply } from './native-harness';

vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: vi.fn(),
}));

const flush = async () => {
  for (let i = 0; i < 50; i++) await Promise.resolve();
};
let state: WorkspaceState;
let manager: TerminalSessionManager;
let calls: { command: string; request: Record<string, unknown> }[];

const renderer = (): TerminalRenderer => ({
  fit: vi.fn(() => ({ rows: 30, cols: 100 })),
  write: vi.fn(async () => true),
  input: vi.fn(() => vi.fn()),
  setInput: vi.fn(),
  setActive: vi.fn(),
  focus: vi.fn(),
  dispose: vi.fn(),
});

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
  state = { sessions: [], tabs: [], creating: false };
  mockIPC((command, args) => {
    const request =
      (args as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'terminal_read_output') return new Promise(() => undefined);
    if (command === 'terminal_resize') return reply().terminal;
    return reply().terminal;
  });
  manager = new TerminalSessionManager(
    (value) => {
      state = value;
    },
    (host, update, adopted) =>
      new TerminalController(host, renderer(), update, adopted),
  );
});
afterEach(async () => {
  await manager.dispose();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('opens an agent the desktop started as a tab that reads its terminal and never creates one', async () => {
  const started = reply();
  adoptTerminal(started.terminal);
  const id = manager.createNative(started)!;
  manager.attach(id, document.createElement('div'));
  await flush();
  expect(calls.filter((c) => c.command === 'create_terminal')).toHaveLength(0);
  const tab = state.sessions.find((s) => s.id === id)!;
  expect(tab.title).toBe('Claude Code · u');
  expect(tab.native).toMatchObject({
    sessionId: started.session.id,
    agent: 'Claude Code',
    throughShell: true,
  });
  expect(tab.view.state).toBe('running');
  expect(tab.view.info?.terminal_id).toBe(started.terminal.terminal_id);
  // The window can find the tab of a session, and only of that one.
  expect(manager.tabOf(started.session.id)).toBe(id);
  expect(manager.tabOf('f'.repeat(32))).toBeUndefined();
  // A resize reaches the agent's terminal: it is fitted like any other.
  await vi.advanceTimersByTimeAsync(100);
  const resize = calls.find((c) => c.command === 'terminal_resize');
  expect(resize?.request).toMatchObject({
    terminal_id: started.terminal.terminal_id,
    size: { rows: 30, cols: 100 },
  });
});

it('closing the tab of an agent closes its terminal, which ends the process', async () => {
  const started = reply();
  adoptTerminal(started.terminal);
  const id = manager.createNative(started)!;
  manager.attach(id, document.createElement('div'));
  await flush();
  await manager.close(id);
  expect(calls.some((c) => c.command === 'close_terminal')).toBe(true);
  expect(state.sessions).toHaveLength(0);
});

it('counts an agent against the same limit as any terminal', async () => {
  for (let n = 0; n < 16; n++) {
    const started = reply({ id: n.toString(16).padStart(32, '0') });
    adoptTerminal(started.terminal);
    expect(manager.createNative(started)).toBeDefined();
  }
  expect(manager.createNative(reply())).toBeUndefined();
  expect(state.error).toContain('limit');
});

it('shows a tab whose terminal the desktop closed (Stop) as closed, not as a failure', async () => {
  const started = reply();
  adoptTerminal(started.terminal);
  mockIPC((command) => {
    if (command === 'terminal_read_output')
      return Promise.reject({ code: 'already_closed' });
    return started.terminal;
  });
  const id = manager.createNative(started)!;
  manager.attach(id, document.createElement('div'));
  await flush();
  await vi.advanceTimersByTimeAsync(50);
  const tab = state.sessions.find((s) => s.id === id)!;
  expect(tab.view.state).toBe('closed');
  expect(tab.view.error).toBeUndefined();
});
