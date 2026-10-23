import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalController,
  type TerminalViewState,
} from '../src/lib/features/terminal/controller';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import { parseUuid } from '../src/lib/contracts/uuid';
import type {
  TerminalInfo,
  TerminalOutputWire,
} from '../src/lib/contracts/terminal';

const id = parseTerminalId('11111111-1111-4111-8111-111111111111');
const receipt = parseUuid('22222222-2222-4222-8222-222222222222');
const info: TerminalInfo = {
  terminal_id: id,
  shell: '/bin/fish',
  cwd: '/example',
  observed_pid: 123,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};
const output = (data: number[] = [], eof = false): TerminalOutputWire => ({
  batch_id: data.length ? receipt : null,
  data,
  eof,
  truncated: false,
  status: eof
    ? { ...info, state: 'exited', exit: { code: 0, signal: null } }
    : info,
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { resolve, promise };
}
const flush = async () => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};
class Observer {
  static instances: Observer[] = [];
  disconnect = vi.fn();
  observe = vi.fn();
  constructor(readonly callback: () => void) {
    Observer.instances.push(this);
  }
}
function renderer() {
  let text!: (v: string) => void;
  let binary!: (v: string) => void;
  const cleanup = vi.fn();
  const value: TerminalRenderer = {
    fit: vi.fn(() => info.size),
    write: vi.fn(async () => true),
    input: vi.fn((t, b) => {
      text = t;
      binary = b;
      return cleanup;
    }),
    setInput: vi.fn(),
    setActive: vi.fn(),
    focus: vi.fn(),
    dispose: vi.fn(),
  };
  return {
    value,
    cleanup,
    text: (v: string) => text(v),
    binary: (v: string) => binary(v),
  };
}
let controller: TerminalController;
let states: TerminalViewState[];
let render: ReturnType<typeof renderer>;
let calls: [string, Record<string, unknown> | undefined][];
let handle: (command: string, args?: Record<string, unknown>) => unknown;
function setup() {
  render = renderer();
  states = [];
  calls = [];
  handle = (command) =>
    command === 'terminal_read_output'
      ? new Promise(() => undefined)
      : command === 'terminal_write'
        ? 1
        : command === 'close_terminal'
          ? { ...info, state: 'closed' }
          : info;
  mockIPC((command, args) => {
    const record = args as Record<string, unknown> | undefined;
    calls.push([command, record]);
    return handle(command, record);
  });
  controller = new TerminalController(
    document.createElement('div'),
    render.value,
    (state) => states.push(state),
  );
}
beforeEach(() => {
  vi.useFakeTimers();
  Observer.instances = [];
  vi.stubGlobal('ResizeObserver', Observer);
  setup();
});
afterEach(async () => {
  await controller.dispose();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const count = (command: string) =>
  calls.filter(([name]) => name === command).length;

describe('real IPC bridge plus terminal lifecycle controller', () => {
  it('initializes once and retains the exact TerminalId', async () => {
    const first = controller.start();
    expect(controller.start()).toBe(first);
    await first;
    expect(count('create_terminal')).toBe(1);
    expect(render.value.fit).toHaveBeenCalled();
    expect(render.value.focus).toHaveBeenCalledTimes(1);
    expect(states.at(-1)?.info?.terminal_id).toBe(id);
  });
  it('passes split bytes unchanged and ACKs only after renderer processing', async () => {
    const processed = deferred<boolean>();
    let reads = 0;
    vi.mocked(render.value.write).mockReturnValueOnce(processed.promise);
    handle = (command) =>
      command === 'terminal_read_output'
        ? ++reads === 1
          ? output([0xe2])
          : output([0x82, 0xac, 0x1b])
        : info;
    await controller.start();
    await flush();
    expect(render.value.write).toHaveBeenCalledWith(Uint8Array.of(0xe2));
    expect(count('terminal_read_output')).toBe(1);
    // Hold the next renderer write too, preventing any accumulated output queue.
    const next = deferred<boolean>();
    vi.mocked(render.value.write).mockReturnValueOnce(next.promise);
    processed.resolve(true);
    await flush();
    expect(count('terminal_read_output')).toBe(2);
    expect(
      calls
        .filter(([c]) => c === 'terminal_read_output')
        .map(([, args]) => args),
    ).toEqual([
      { request: { terminal_id: id, ack: null } },
      { request: { terminal_id: id, ack: receipt } },
    ]);
    expect(render.value.write).toHaveBeenLastCalledWith(
      Uint8Array.of(0x82, 0xac, 0x1b),
    );
  });
  it('sends keyboard text and binary replies independently of blocked output', async () => {
    handle = (command, args) =>
      command === 'terminal_read_output'
        ? new Promise(() => undefined)
        : command === 'terminal_write'
          ? (args!.request as { data: number[] }).data.length
          : info;
    await controller.start();
    render.text('é\r\u0003');
    render.binary('\xff');
    await flush();
    expect(
      calls.filter(([c]) => c === 'terminal_write').map(([, args]) => args),
    ).toEqual([
      { request: { terminal_id: id, data: [0xc3, 0xa9, 13, 3] } },
      { request: { terminal_id: id, data: [255] } },
    ]);
  });
  it('chunks paste to <=4096 bytes with one input write in flight', async () => {
    const first = deferred<number>();
    let writes = 0;
    handle = (command, args) =>
      command === 'terminal_read_output'
        ? new Promise(() => undefined)
        : command === 'terminal_write'
          ? ++writes === 1
            ? first.promise
            : (args!.request as { data: number[] }).data.length
          : info;
    await controller.start();
    render.text('x'.repeat(9000));
    await flush();
    expect(writes).toBe(1);
    first.resolve(4096);
    await flush();
    expect(
      calls
        .filter(([c]) => c === 'terminal_write')
        .map(([, args]) => (args!.request as { data: number[] }).data.length),
    ).toEqual([4096, 4096, 808]);
  });
  it('rejects a saturated input queue visibly rather than buffering forever', async () => {
    await controller.start();
    render.text('x'.repeat(65537));
    await flush();
    expect(states.at(-1)).toMatchObject({
      state: 'failed',
      error: 'input_limit',
    });
    expect(count('terminal_write')).toBe(0);
    expect(count('close_terminal')).toBe(1);
  });
  it('coalesces observer notifications and skips identical sizes', async () => {
    await controller.start();
    await vi.advanceTimersByTimeAsync(60);
    expect(count('terminal_resize')).toBe(0);
    vi.mocked(render.value.fit).mockReturnValue({ rows: 37, cols: 111 });
    for (let i = 0; i < 100; i++) Observer.instances[0].callback();
    await vi.advanceTimersByTimeAsync(60);
    expect(count('terminal_resize')).toBe(1);
    expect(calls.find(([c]) => c === 'terminal_resize')?.[1]).toEqual({
      request: { terminal_id: id, size: { rows: 37, cols: 111 } },
    });
    Observer.instances[0].callback();
    await vi.advanceTimersByTimeAsync(60);
    expect(count('terminal_resize')).toBe(1);
  });
  it('bounds resize concurrency to one operation plus latest desired size', async () => {
    const pending = deferred<TerminalInfo>();
    let sizes = 0;
    handle = (command) =>
      command === 'terminal_resize' && ++sizes === 1
        ? pending.promise
        : command === 'terminal_read_output'
          ? new Promise(() => undefined)
          : info;
    await controller.start();
    vi.mocked(render.value.fit).mockReturnValue({ rows: 30, cols: 100 });
    await vi.advanceTimersByTimeAsync(60);
    vi.mocked(render.value.fit).mockReturnValue({ rows: 40, cols: 120 });
    Observer.instances[0].callback();
    await vi.advanceTimersByTimeAsync(60);
    vi.mocked(render.value.fit).mockReturnValue({ rows: 50, cols: 140 });
    Observer.instances[0].callback();
    await vi.advanceTimersByTimeAsync(60);
    expect(sizes).toBe(1);
    pending.resolve(info);
    await flush();
    expect(sizes).toBe(2);
    expect(calls.filter(([c]) => c === 'terminal_resize').at(-1)?.[1]).toEqual({
      request: { terminal_id: id, size: { rows: 50, cols: 140 } },
    });
  });
  it('backs off idle pulls and wakes output immediately on input', async () => {
    handle = (command) =>
      command === 'terminal_read_output'
        ? output()
        : command === 'terminal_write'
          ? 1
          : info;
    await controller.start();
    await vi.advanceTimersByTimeAsync(2000);
    const before = count('terminal_read_output');
    expect(before).toBeLessThan(12);
    render.text('x');
    await flush();
    expect(count('terminal_read_output')).toBe(before + 1);
  });
  it('does not pace a pull the desktop already waited for, and still paces one that came back at once', async () => {
    // A desktop that waits a second for output before answering empty: the page asks again at
    // once, so at rest it asks about once a second and not three times.
    handle = (command) =>
      command === 'terminal_read_output'
        ? new Promise((resolve) => setTimeout(() => resolve(output()), 1000))
        : command === 'terminal_write'
          ? 1
          : info;
    await controller.start();
    await vi.advanceTimersByTimeAsync(10_000);
    const waiting = count('terminal_read_output');
    expect(waiting).toBeGreaterThanOrEqual(9);
    expect(waiting).toBeLessThanOrEqual(11);
    // Input still reaches the program at once, and the pending pull is not made to wait for it.
    render.text('x');
    await flush();
    expect(count('terminal_write')).toBe(1);
  });
  it('drains all exit batches, disables input, stops polling and never respawns', async () => {
    let reads = 0;
    handle = (command) =>
      command === 'terminal_read_output'
        ? output(++reads < 3 ? [reads] : [], true)
        : info;
    await controller.start();
    await flush();
    expect(render.value.write).toHaveBeenCalledTimes(2);
    expect(states.at(-1)?.state).toBe('exited');
    render.text('ignored');
    await vi.advanceTimersByTimeAsync(10000);
    expect(count('terminal_read_output')).toBe(3);
    expect(count('terminal_write')).toBe(0);
    expect(count('create_terminal')).toBe(1);
    expect(count('close_terminal')).toBe(0);
    expect(Observer.instances[0].disconnect).toHaveBeenCalledTimes(1);
  });
  it('waits for the backend exit snapshot when PTY EOF arrives before child reaping', async () => {
    let reads = 0;
    handle = (command) =>
      command === 'terminal_read_output'
        ? ++reads === 1
          ? { ...output([], true), status: info }
          : output([], true)
        : info;
    await controller.start();
    await flush();
    await vi.advanceTimersByTimeAsync(1000);
    expect(count('terminal_read_output')).toBe(2);
    expect(states.at(-1)?.info?.exit?.code).toBe(0);
    expect(states.at(-1)?.state).toBe('exited');
    await vi.advanceTimersByTimeAsync(10000);
    expect(count('terminal_read_output')).toBe(2);
  });
  it('disposes once, cancels pending render and removes observers/listeners/timers', async () => {
    const pending = deferred<boolean>();
    vi.mocked(render.value.write).mockReturnValue(pending.promise);
    handle = (command) =>
      command === 'terminal_read_output' ? output([1]) : info;
    await controller.start();
    await flush();
    await Promise.all([
      controller.close(),
      controller.dispose(),
      controller.dispose(),
    ]);
    expect(count('close_terminal')).toBe(1);
    expect(render.value.dispose).toHaveBeenCalledTimes(1);
    expect(render.cleanup).toHaveBeenCalledTimes(1);
    expect(Observer.instances[0].disconnect).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(10000);
    expect(count('terminal_read_output')).toBe(1);
  });
  it('closes a late create exactly once after component disposal', async () => {
    const create = deferred<TerminalInfo>();
    handle = (command) =>
      command === 'create_terminal' ? create.promise : info;
    const starting = controller.start();
    const disposal = controller.dispose();
    create.resolve(info);
    await Promise.all([starting, disposal]);
    expect(count('create_terminal')).toBe(1);
    expect(count('close_terminal')).toBe(1);
    expect(count('terminal_read_output')).toBe(0);
    expect(states.at(-1)?.state).toBe('starting');
  });
  it('reports create failure safely and leaves no loading loop', async () => {
    handle = () =>
      Promise.reject({ code: 'spawn_failed', diagnostic: 'private detail' });
    await controller.start();
    expect(states.at(-1)).toMatchObject({
      state: 'failed',
      error: 'spawn_failed',
    });
    expect(JSON.stringify(states)).not.toContain('private detail');
    expect(count('close_terminal')).toBe(0);
  });
  it('fails ambiguous output IPC once without blindly retrying ACK', async () => {
    handle = (command) =>
      command === 'terminal_read_output'
        ? Promise.reject(new Error('transport detail'))
        : info;
    await controller.start();
    await flush();
    await vi.advanceTimersByTimeAsync(5000);
    expect(states.at(-1)).toMatchObject({
      state: 'failed',
      error: 'terminal_error',
    });
    expect(count('terminal_read_output')).toBe(1);
    expect(count('close_terminal')).toBe(1);
  });
  it('does not retry partial write errors', async () => {
    handle = (command) =>
      command === 'terminal_write'
        ? Promise.reject({ code: 'write_failed', bytes_written: 1 })
        : command === 'terminal_read_output'
          ? new Promise(() => undefined)
          : info;
    await controller.start();
    render.text('abc');
    await flush();
    expect(count('terminal_write')).toBe(1);
    expect(states.at(-1)).toMatchObject({
      state: 'failed',
      error: 'write_failed',
    });
  });
  it('offers a write the host did not take (busy) again and keeps the terminal', async () => {
    // Another window was writing at that moment: nothing of this write reached the terminal.
    let tries = 0;
    handle = (command) => {
      if (command === 'terminal_write') {
        tries += 1;
        return tries <= 3 ? Promise.reject({ code: 'busy' }) : 3;
      }
      return command === 'terminal_read_output'
        ? new Promise(() => undefined)
        : info;
    };
    await controller.start();
    render.text('abc');
    await flush();
    await vi.advanceTimersByTimeAsync(200);
    expect(count('terminal_write')).toBe(4);
    expect(states.at(-1)?.state).toBe('running');
    expect(count('close_terminal')).toBe(0);
  });
  it('gives up on a terminal that stays busy, as before', async () => {
    handle = (command) =>
      command === 'terminal_write'
        ? Promise.reject({ code: 'busy' })
        : command === 'terminal_read_output'
          ? new Promise(() => undefined)
          : info;
    await controller.start();
    render.text('abc');
    await flush();
    await vi.advanceTimersByTimeAsync(5000);
    expect(count('terminal_write')).toBe(41);
    expect(states.at(-1)).toMatchObject({ state: 'failed', error: 'busy' });
  });
  it('does not write after disposal even when an input promise resolves late', async () => {
    const write = deferred<number>();
    handle = (command) =>
      command === 'terminal_write'
        ? write.promise
        : command === 'terminal_read_output'
          ? new Promise(() => undefined)
          : info;
    await controller.start();
    render.text('x'.repeat(5000));
    await controller.dispose();
    write.resolve(4096);
    await flush();
    expect(count('terminal_write')).toBe(1);
    expect(count('close_terminal')).toBe(1);
  });
});
