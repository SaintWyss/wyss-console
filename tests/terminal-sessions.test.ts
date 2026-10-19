import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalSessionManager,
  type WorkspaceState,
} from '../src/lib/features/terminal/session-manager';
import { TerminalController } from '../src/lib/features/terminal/controller';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import type {
  TerminalInfo,
  TerminalOutputWire,
} from '../src/lib/contracts/terminal';
import {
  parseTerminalId,
  type ConsoleSessionId,
  type TerminalId,
} from '../src/lib/contracts/identities';
import { parseUuid } from '../src/lib/contracts/uuid';

vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: vi.fn(),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const flush = async () => {
  for (let i = 0; i < 50; i++) await Promise.resolve();
};
class Observer {
  static instances: Observer[] = [];
  constructor(readonly callback: () => void) {
    Observer.instances.push(this);
  }
  observe = vi.fn();
  disconnect = vi.fn();
}
function renderer() {
  let input!: (text: string) => void;
  let active = true;
  let enabled = false;
  const value: TerminalRenderer = {
    fit: vi.fn(() => ({ rows: 24, cols: 80 })),
    write: vi.fn(async () => true),
    input: vi.fn((text) => {
      input = text;
      return vi.fn();
    }),
    setInput: vi.fn((v) => {
      enabled = v;
    }),
    setActive: vi.fn((v) => {
      active = v;
    }),
    focus: vi.fn(),
    dispose: vi.fn(),
  };
  return {
    value,
    type: (text: string) => {
      if (active && enabled) input(text);
    },
  };
}
let manager: TerminalSessionManager;
let state: WorkspaceState;
let renders: ReturnType<typeof renderer>[];
let infos: TerminalInfo[];
let calls: { command: string; request: Record<string, unknown> }[];
let pending: Map<TerminalId, ReturnType<typeof deferred<TerminalOutputWire>>>;
let createDelay: ReturnType<typeof deferred<TerminalInfo>> | undefined;
let writeDelay: ReturnType<typeof deferred<number>> | undefined;
let createError: boolean;
const count = (command: string) => calls.filter((c) => c.command === command);
const info = (ordinal: number): TerminalInfo => ({
  terminal_id: parseTerminalId(
    `00000000-0000-4000-8000-${String(ordinal).padStart(12, '0')}`,
  ),
  shell: '/bin/fish',
  cwd: '/example',
  observed_pid: 100 + ordinal,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
});
function attach(id?: ConsoleSessionId) {
  manager.attach(
    id ?? state.sessions.at(-1)!.id,
    document.createElement('div'),
  );
}
async function startup() {
  manager.start();
  attach();
  await flush();
}
async function create() {
  const id = manager.create()!;
  attach(id);
  await flush();
  return id;
}
function deliver(index: number, bytes: number[], exit = false) {
  const value = infos[index];
  pending.get(value.terminal_id)!.resolve({
    batch_id: bytes.length
      ? parseUuid('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa')
      : null,
    data: bytes,
    eof: exit,
    truncated: false,
    status: exit
      ? { ...value, state: 'exited', exit: { code: 0, signal: null } }
      : value,
  });
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('ResizeObserver', Observer);
  Observer.instances = [];
  renders = [];
  infos = [];
  calls = [];
  pending = new Map();
  createDelay = undefined;
  writeDelay = undefined;
  createError = false;
  state = { sessions: [], tabs: [], creating: false };
  mockIPC((command, args) => {
    const request =
      (args as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'create_terminal') {
      if (createError)
        return Promise.reject({ code: 'spawn_failed', diagnostic: 'private' });
      const value = info(infos.length + 1);
      infos.push(value);
      return createDelay?.promise ?? value;
    }
    const id = request.terminal_id as TerminalId;
    if (command === 'terminal_read_output') {
      const read = deferred<TerminalOutputWire>();
      pending.set(id, read);
      return read.promise;
    }
    if (command === 'terminal_write')
      return writeDelay?.promise ?? (request.data as number[]).length;
    return infos.find((v) => v.terminal_id === id);
  });
  manager = new TerminalSessionManager(
    (value) => {
      state = value;
    },
    (host, update) => {
      const render = renderer();
      renders.push(render);
      return new TerminalController(host, render.value, update);
    },
  );
});
afterEach(async () => {
  await manager.dispose();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('startup is idempotent and creates exactly one terminal', async () => {
  await startup();
  manager.start();
  attach();
  await flush();
  expect(count('create_terminal')).toHaveLength(1);
  expect(renders).toHaveLength(1);
});
it('allocates distinct local identities, exact TerminalIds and insertion titles', async () => {
  await startup();
  await create();
  expect(new Set(state.sessions.map((s) => s.id)).size).toBe(2);
  expect(state.sessions.map((s) => s.title)).toEqual([
    'Terminal 1',
    'Terminal 2',
  ]);
  expect(state.sessions.map((s) => s.view.info!.terminal_id)).toEqual(
    infos.map((i) => i.terminal_id),
  );
  expect(state.sessions[0].id).not.toBe(infos[0].terminal_id);
});
it('creating a second terminal does not recreate the first', async () => {
  await startup();
  const first = renders[0];
  await create();
  expect(renders[0]).toBe(first);
  expect(count('create_terminal')).toHaveLength(2);
  expect(first.value.dispose).not.toHaveBeenCalled();
});
it('activation changes selection while retaining each renderer and resource', async () => {
  await startup();
  await create();
  const ids = state.sessions.map((s) => s.id);
  manager.activate(ids[0]);
  manager.activate(ids[1]);
  manager.activate(ids[0]);
  expect(state.active).toBe(ids[0]);
  expect(renders).toHaveLength(2);
  expect(count('close_terminal')).toHaveLength(0);
});
it('interleaved A/B output reaches only its owning renderer including inactive A', async () => {
  await startup();
  await create();
  deliver(1, [66]);
  await flush();
  deliver(0, [65]);
  await flush();
  expect(renders[0].value.write).toHaveBeenCalledExactlyOnceWith(
    Uint8Array.of(65),
  );
  expect(renders[1].value.write).toHaveBeenCalledExactlyOnceWith(
    Uint8Array.of(66),
  );
  expect(state.active).toBe(state.sessions[1].id);
});
it('both controllers continue bounded ACK flow while their tab is inactive', async () => {
  await startup();
  await create();
  const processed = deferred<boolean>();
  vi.mocked(renders[0].value.write).mockReturnValueOnce(processed.promise);
  deliver(0, [0xe2]);
  await flush();
  expect(
    count('terminal_read_output').filter(
      (c) => c.request.terminal_id === infos[0].terminal_id,
    ),
  ).toHaveLength(1);
  processed.resolve(true);
  await flush();
  expect(
    count('terminal_read_output')
      .filter((c) => c.request.terminal_id === infos[0].terminal_id)
      .at(-1)!.request.ack,
  ).not.toBeNull();
  deliver(0, [0x82, 0xac]);
  await flush();
  expect(renders[0].value.write).toHaveBeenLastCalledWith(
    Uint8Array.of(0x82, 0xac),
  );
});
it('input A and B use their exact owning TerminalIds', async () => {
  await startup();
  renders[0].type('A');
  await flush();
  await create();
  renders[1].type('B');
  await flush();
  expect(count('terminal_write').map((c) => c.request)).toEqual([
    { terminal_id: infos[0].terminal_id, data: [65] },
    { terminal_id: infos[1].terminal_id, data: [66] },
  ]);
});
it('rapid switches cannot redirect a delayed/chunked input write', async () => {
  await startup();
  writeDelay = deferred<number>();
  renders[0].type('A'.repeat(5000));
  await flush();
  await create();
  const [a, b] = state.sessions;
  manager.activate(a.id);
  manager.activate(b.id);
  writeDelay.resolve(4096);
  writeDelay = undefined;
  await flush();
  expect(count('terminal_write')).toHaveLength(2);
  expect(
    count('terminal_write').every(
      (c) => c.request.terminal_id === infos[0].terminal_id,
    ),
  ).toBe(true);
});
it('inactive user input is gated and no hidden focus is requested', async () => {
  await startup();
  await create();
  const before = count('terminal_write').length;
  renders[0].type('hidden');
  expect(count('terminal_write')).toHaveLength(before);
  expect(renders[0].value.setActive).toHaveBeenLastCalledWith(false);
});
it('hidden renderers do not fit or resize even when an observer fires', async () => {
  await startup();
  await create();
  const before = vi.mocked(renders[0].value.fit).mock.calls.length;
  vi.mocked(renders[0].value.fit).mockReturnValue({ rows: 1, cols: 2 });
  Observer.instances[0].callback();
  await vi.advanceTimersByTimeAsync(60);
  expect(renders[0].value.fit).toHaveBeenCalledTimes(before);
  expect(
    count('terminal_resize').filter(
      (c) => c.request.terminal_id === infos[0].terminal_id,
    ),
  ).toHaveLength(0);
});
it('activation performs a fresh fit, resize and focus after visibility changes', async () => {
  await startup();
  await create();
  vi.mocked(renders[0].value.fit).mockReturnValue({ rows: 40, cols: 120 });
  manager.activate(state.sessions[0].id);
  await vi.advanceTimersByTimeAsync(60);
  expect(count('terminal_resize')).toContainEqual({
    command: 'terminal_resize',
    request: {
      terminal_id: infos[0].terminal_id,
      size: { rows: 40, cols: 120 },
    },
  });
  expect(renders[0].value.focus).toHaveBeenCalled();
});
it('closing an inactive session leaves active controller untouched', async () => {
  await startup();
  await create();
  const active = state.active;
  await manager.close(state.sessions[0].id);
  expect(state.active).toBe(active);
  expect(renders[1].value.dispose).not.toHaveBeenCalled();
  expect(count('close_terminal')[0].request.terminal_id).toBe(
    infos[0].terminal_id,
  );
});
it('closing active middle tab chooses its right neighbor then left at the end', async () => {
  await startup();
  await create();
  await create();
  const [a, b, c] = state.sessions;
  manager.activate(b.id);
  await manager.close(b.id);
  expect(state.active).toBe(c.id);
  await manager.close(c.id);
  expect(state.active).toBe(a.id);
});
it('closing the final terminal leaves empty workspace without automatic respawn', async () => {
  await startup();
  await manager.close(state.sessions[0].id);
  await vi.advanceTimersByTimeAsync(10000);
  expect(state.sessions).toHaveLength(0);
  expect(state.active).toBeUndefined();
  expect(count('create_terminal')).toHaveLength(1);
});
it('an empty workspace can explicitly create a fresh terminal', async () => {
  await startup();
  await manager.close(state.sessions[0].id);
  await create();
  expect(state.sessions).toHaveLength(1);
  expect(state.sessions[0].title).toBe('Terminal 2');
});
it('natural exit preserves tab and final renderer screen, disables input and stops pulls', async () => {
  await startup();
  await create();
  deliver(0, [70], true);
  await flush();
  deliver(0, [], true);
  await flush();
  expect(state.sessions[0].view.state).toBe('exited');
  expect(state.sessions).toHaveLength(2);
  expect(renders[0].value.dispose).not.toHaveBeenCalled();
  manager.activate(state.sessions[0].id);
  renders[0].type('ignored');
  expect(count('terminal_write')).toHaveLength(0);
  const before = count('terminal_read_output').length;
  await vi.advanceTimersByTimeAsync(10000);
  expect(count('terminal_read_output')).toHaveLength(before);
});
it('closing an exited tab disposes its retained renderer without affecting another shell', async () => {
  await startup();
  await create();
  deliver(0, [], true);
  await flush();
  await manager.close(state.sessions[0].id);
  expect(renders[0].value.dispose).toHaveBeenCalledTimes(1);
  expect(state.sessions[0].view.state).toBe('running');
});
it('a failed creation is visible and isolated; subsequent creation still works', async () => {
  await startup();
  createError = true;
  await create();
  expect(state.sessions[1].view).toMatchObject({
    state: 'failed',
    error: 'spawn_failed',
  });
  expect(state.sessions[0].view.state).toBe('running');
  createError = false;
  await create();
  expect(state.sessions[2].view.state).toBe('running');
  expect(JSON.stringify(state)).not.toContain('private');
});
it('renderer failure remains local and can be closed', async () => {
  await manager.dispose();
  manager = new TerminalSessionManager(
    (v) => {
      state = v;
    },
    () => {
      throw new Error('private');
    },
  );
  await startup();
  expect(state.sessions[0].view).toEqual({
    state: 'failed',
    error: 'renderer_error',
  });
  await manager.close(state.sessions[0].id);
  expect(state.sessions).toHaveLength(0);
});
it('repeated close is idempotent and sends exactly one backend close', async () => {
  await startup();
  const id = state.sessions[0].id;
  const first = manager.close(id);
  expect(manager.close(id)).toBe(first);
  await first;
  await manager.close(id);
  expect(count('close_terminal')).toHaveLength(1);
  expect(renders[0].value.dispose).toHaveBeenCalledTimes(1);
});
it('late create after close is cleaned once without ghost tab or output loop', async () => {
  createDelay = deferred<TerminalInfo>();
  manager.start();
  attach();
  await flush();
  const id = state.sessions[0].id;
  const closing = manager.close(id);
  createDelay.resolve(infos[0]);
  await closing;
  expect(state.sessions).toHaveLength(0);
  expect(count('close_terminal')).toHaveLength(1);
  expect(count('terminal_read_output')).toHaveLength(0);
});
it('close before DOM attachment creates no PTY and cannot be revived by late attachment', async () => {
  manager.start();
  const id = state.sessions[0].id;
  await manager.close(id);
  attach(id);
  expect(count('create_terminal')).toHaveLength(0);
  expect(renders).toHaveLength(0);
});
it('close cancels pending output and renderer processing without another pull', async () => {
  await startup();
  const rendering = deferred<boolean>();
  vi.mocked(renders[0].value.write).mockReturnValueOnce(rendering.promise);
  deliver(0, [1]);
  await flush();
  await manager.close(state.sessions[0].id);
  rendering.resolve(true);
  await flush();
  expect(count('terminal_read_output')).toHaveLength(1);
  expect(renders[0].value.dispose).toHaveBeenCalledTimes(1);
});
it('close during pending input prevents remaining chunks from writing', async () => {
  await startup();
  writeDelay = deferred<number>();
  renders[0].type('A'.repeat(5000));
  await flush();
  await manager.close(state.sessions[0].id);
  writeDelay.resolve(4096);
  await flush();
  expect(count('terminal_write')).toHaveLength(1);
});
it('manager disposal cleans all sessions once and disconnects all observers', async () => {
  await startup();
  await create();
  await create();
  const cleanup = manager.dispose();
  expect(manager.dispose()).toBe(cleanup);
  await cleanup;
  expect(count('close_terminal')).toHaveLength(3);
  for (const render of renders)
    expect(render.value.dispose).toHaveBeenCalledTimes(1);
  for (const observer of Observer.instances)
    expect(observer.disconnect).toHaveBeenCalledTimes(1);
  expect(manager.create()).toBeUndefined();
});
it('serializes creation admission during double clicks and pending create', async () => {
  createDelay = deferred<TerminalInfo>();
  manager.start();
  attach();
  manager.create();
  manager.create();
  expect(state.sessions).toHaveLength(1);
  expect(count('create_terminal')).toHaveLength(1);
  createDelay.resolve(infos[0]);
  await flush();
  expect(state.creating).toBe(false);
});
it('respects 16 slots including exited sessions and keeps all resources on limit failure', async () => {
  await startup();
  for (let i = 1; i < 16; i++) await create();
  const active = state.active;
  expect(manager.create()).toBeUndefined();
  expect(state.error).toContain('limit reached');
  expect(state.sessions).toHaveLength(16);
  expect(state.active).toBe(active);
  expect(count('create_terminal')).toHaveLength(16);
  expect(count('close_terminal')).toHaveLength(0);
  await manager.close(state.sessions[0].id);
  await create();
  expect(state.sessions).toHaveLength(16);
});

it('activating the already selected tab returns focus to its terminal', async () => {
  await startup();
  await vi.advanceTimersByTimeAsync(60);
  const before = vi.mocked(renders[0].value.focus).mock.calls.length;
  manager.activate(state.sessions[0].id);
  await vi.advanceTimersByTimeAsync(60);
  expect(renders[0].value.focus).toHaveBeenCalledTimes(before + 1);
  expect(count('create_terminal')).toHaveLength(1);
});
