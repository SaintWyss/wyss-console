import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalSessionManager,
  type WorkspaceState,
} from '../src/lib/features/terminal/session-manager';
import { TerminalController } from '../src/lib/features/terminal/controller';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { leaves, leaf, split } from '../src/lib/features/terminal/layout';
import { leaderAction } from '../src/lib/features/terminal/keymap';

vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: vi.fn(),
}));

const flush = async () => {
  for (let i = 0; i < 80; i++) await Promise.resolve();
};
const terminal = (n: number) => ({
  terminal_id: `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`,
  shell: '/bin/fish',
  cwd: `/home/u/${n}`,
  observed_pid: 100 + n,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
});
const held = (n: number, title = `Held ${n}`, native = null) => ({
  terminal: terminal(n),
  title,
  created_at: n,
  native,
});

let state: WorkspaceState;
let manager: TerminalSessionManager;
let calls: {
  command: string;
  request: Record<string, unknown>;
  args: Record<string, unknown>;
}[];
let created: number;
let hostList: unknown[];
let saved: unknown;
let controllers: {
  active: boolean;
  focus: boolean;
  title?: string;
  cwd?: string;
}[];

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
async function open(persistent = true) {
  manager = new TerminalSessionManager(
    (value) => {
      state = value;
    },
    (host, update, adopted, title, cwd) => {
      const record = { active: true, focus: false, title, cwd };
      controllers.push(record);
      const controller = new TerminalController(
        host,
        renderer(),
        update,
        adopted,
        title,
        cwd,
      );
      const original = controller.setActive.bind(controller);
      controller.setActive = (active: boolean, focus = true) => {
        record.active = active;
        record.focus = focus;
        original(active, focus);
      };
      return controller;
    },
    persistent,
  );
  manager.start();
  await flush();
  attachAll();
  await flush();
}
const count = (c: string) => calls.filter((x) => x.command === c);
const tabOfActive = () => state.tabs.find((t) => t.id === state.activeTab)!;

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
  created = 0;
  hostList = [];
  saved = null;
  controllers = [];
  state = { sessions: [], tabs: [], creating: false };
  mockIPC((command, args) => {
    const request =
      (args as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({
      command,
      request,
      args: (args ?? {}) as Record<string, unknown>,
    });
    if (command === 'host_terminals') return hostList;
    if (command === 'layout_read') return saved;
    if (command === 'layout_write') return null;
    if (command === 'terminal_read_output') return new Promise(() => undefined);
    if (command === 'create_terminal') return terminal(100 + ++created);
    if (command === 'terminal_redraw') return null;
    return terminal(1);
  });
});
afterEach(async () => {
  await manager.dispose();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('splits a pane into a second terminal in the same tab, and the new one has the focus', async () => {
  await open();
  const first = state.sessions[0]!.id;
  const added = manager.split(first, 'row')!;
  attachAll();
  await flush();
  expect(state.tabs).toHaveLength(1);
  expect(leaves(tabOfActive().root)).toEqual([first, added]);
  expect(state.active).toBe(added);
  expect(manager.arrangement(state.activeTab!)!.panes.get(first)).toMatchObject(
    { w: 0.5 },
  );
  expect(count('create_terminal')).toHaveLength(2);
});

it('draws every pane of the active tab, and only one has the keyboard', async () => {
  await open();
  const first = state.sessions[0]!.id;
  manager.split(first, 'row');
  attachAll();
  await flush();
  manager.create(); // a second tab
  attachAll();
  await flush();
  expect(state.tabs).toHaveLength(2);
  // Controllers are in session order: two panes of tab one, one pane of tab two.
  expect(controllers.map((c) => [c.active, c.focus])).toEqual([
    [false, false],
    [false, false],
    [true, true],
  ]);
  manager.activateTab(state.tabs[0]!.id);
  expect(controllers.map((c) => [c.active, c.focus])).toEqual([
    [true, false],
    [true, true],
    [false, false],
  ]);
});

it('closing a pane gives its space to the one beside it and the focus too; the last one closes the tab', async () => {
  await open();
  const a = state.sessions[0]!.id;
  const b = manager.split(a, 'row')!;
  attachAll();
  await flush();
  await manager.close(b);
  expect(leaves(tabOfActive().root)).toEqual([a]);
  expect(state.active).toBe(a);
  const second = manager.create()!;
  attachAll();
  await flush();
  await manager.close(second);
  expect(state.tabs).toHaveLength(1);
  expect(state.activeTab).toBe(state.tabs[0]!.id);
  await manager.closeTab(state.tabs[0]!.id);
  expect(state.tabs).toHaveLength(0);
  expect(state.sessions).toHaveLength(0);
});

it('moves the focus by direction, steps through panes and tabs, and jumps by number', async () => {
  await open();
  const a = state.sessions[0]!.id;
  const b = manager.split(a, 'row')!;
  attachAll();
  await flush();
  manager.focusPane('left');
  expect(state.active).toBe(a);
  manager.focusPane('right');
  expect(state.active).toBe(b);
  manager.focusPane('next');
  expect(state.active).toBe(a);
  manager.focusPane('previous');
  expect(state.active).toBe(b);
  manager.create();
  attachAll();
  await flush();
  expect(state.tabs).toHaveLength(2);
  manager.stepTab(-1);
  expect(state.activeTab).toBe(state.tabs[0]!.id);
  manager.stepTab(-1);
  expect(state.activeTab).toBe(state.tabs[1]!.id);
  manager.jumpTab(1);
  expect(state.activeTab).toBe(state.tabs[0]!.id);
  manager.jumpTab(9);
  expect(state.activeTab).toBe(state.tabs[1]!.id);
});

it('resizes a divider within the minimum and refuses a split past sixteen panes', async () => {
  await open();
  const a = state.sessions[0]!.id;
  manager.split(a, 'row');
  const tab = state.tabs[0]!.id;
  manager.resizeSplit(tab, '', 0.8);
  expect(manager.arrangement(tab)!.panes.get(a)!.w).toBeCloseTo(0.8);
  manager.resizeSplit(tab, '', -3);
  expect(manager.arrangement(tab)!.panes.get(a)!.w).toBeCloseTo(0.1);
  // Fill the tab: every pane split once per round, four rounds.
  for (let round = 0; round < 4; round++)
    for (const id of leaves(state.tabs[0]!.root)) {
      manager.split(id, round % 2 ? 'col' : 'row');
      attachAll();
      await flush();
    }
  expect(leaves(state.tabs[0]!.root).length).toBeLessThanOrEqual(16);
  const before = state.sessions.length;
  manager.split(state.sessions[0]!.id, 'row');
  expect(state.sessions.length).toBe(before);
  expect(state.error).toMatch(/16|limit/);
});

it('writes the layout down a moment after a change, with terminals and titles and nothing else', async () => {
  await open();
  const a = state.sessions[0]!.id;
  manager.split(a, 'col');
  attachAll();
  await flush();
  expect(count('layout_write')).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(500);
  const written = count('layout_write').at(-1)!.args as unknown as {
    layout: {
      version: number;
      tabs: { root: unknown }[];
      terminals: Record<string, { title: string; cwd: string }>;
    };
  };
  expect(written.layout.version).toBe(1);
  expect(written.layout.tabs).toHaveLength(1);
  expect(Object.values(written.layout.terminals).map((t) => t.title)).toEqual([
    'Terminal 1',
    'Terminal 2',
  ]);
});

it('restores what the host holds in the saved layout, makes again a shell that is gone, drops an agent that is gone, and gives the rest a tab each', async () => {
  hostList = [held(1), held(2), held(3), held(4)];
  const t = (n: number) => terminal(n).terminal_id;
  saved = {
    version: 1,
    active: 1,
    tabs: [
      { root: split(leaf(t(1)), t(1), t(2), 'row'), focus: t(2) },
      {
        root: split(
          split(leaf(t(3)), t(3), 'gone-shell', 'col'),
          t(3),
          'gone-agent',
          'row',
        ),
        focus: t(3),
      },
    ],
    terminals: {
      [t(1)]: { title: 'One', cwd: '/a' },
      [t(2)]: { title: 'Two', cwd: '/b' },
      [t(3)]: { title: 'Three', cwd: '/c' },
      'gone-shell': { title: 'Logs', cwd: '/var/log' },
      'gone-agent': { title: 'Claude Code · app', cwd: '/x', native: true },
    },
  };
  await open();
  // Two saved tabs and the unplaced terminal (4) in its own tab.
  expect(state.tabs).toHaveLength(3);
  expect(state.tabs.map((x) => leaves(x.root).length)).toEqual([2, 2, 1]);
  expect(state.activeTab).toBe(state.tabs[1]!.id);
  // The shell that was gone is made again with its title and folder; nothing else is made.
  const makes = count('create_terminal');
  expect(makes.map((c) => [c.request.title, c.request.cwd])).toEqual([
    ['Logs', '/var/log'],
  ]);
  expect(state.sessions.map((s) => s.title)).toEqual(
    expect.arrayContaining(['Held 1', 'Held 2', 'Held 3', 'Held 4', 'Logs']),
  );
});

it('does not touch the host or the layout when it is not persistent', async () => {
  await open(false);
  expect(count('host_terminals')).toHaveLength(0);
  expect(count('layout_read')).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(1000);
  expect(count('layout_write')).toHaveLength(0);
});

it('takes the Console’s chords by the physical key and leaves every other key to the terminal', () => {
  const key = (code: string, over: Record<string, boolean> = {}) =>
    leaderAction({
      ctrlKey: true,
      shiftKey: true,
      altKey: false,
      metaKey: false,
      code,
      ...over,
    });
  expect(key('KeyT')).toEqual({ do: 'new-tab' });
  expect(key('KeyD')).toEqual({ do: 'split', dir: 'row' });
  expect(key('KeyE')).toEqual({ do: 'split', dir: 'col' });
  expect(key('KeyF')).toEqual({ do: 'search' });
  expect(key('ArrowLeft')).toEqual({ do: 'focus', way: 'left' });
  expect(key('Digit3')).toEqual({ do: 'jump', n: 3 });
  expect(key('PageDown')).toEqual({ do: 'tab', step: 1 });
  // Copy, paste and the input method's unicode entry are not taken.
  for (const code of ['KeyC', 'KeyV', 'KeyU', 'KeyA', 'Digit0'])
    expect(key(code)).toBeUndefined();
  // Not without the whole chord.
  expect(key('KeyT', { shiftKey: false })).toBeUndefined();
  expect(key('KeyT', { ctrlKey: false })).toBeUndefined();
  expect(key('KeyT', { altKey: true })).toBeUndefined();
  expect(key('KeyT', { metaKey: true })).toBeUndefined();
});
