import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  TerminalSessionManager,
  type WorkspaceState,
} from '../src/lib/features/terminal/session-manager';
import { TerminalController } from '../src/lib/features/terminal/controller';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseAttach } from '../src/lib/contracts/terminal';
import { parserNote } from '../src/lib/features/terminal/parser-status';

vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: vi.fn(),
}));

const flush = async () => {
  for (let i = 0; i < 80; i++) await Promise.resolve();
};
const id = '00000000-0000-4000-8000-000000000001';
const info = {
  terminal_id: id,
  shell: '/bin/bash',
  cwd: '/work',
  observed_pid: 101,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};
const b64 = (text: string) => btoa(text);
const parser = (extra: Record<string, unknown> = {}) => ({
  health: 'healthy',
  fidelity: 'exact',
  restarts: 0,
  reason: null,
  missing_bytes: 0,
  skipped_bytes: 0,
  last_failure: null,
  sandbox: { layers: 15, landlock_abi: 6 },
  ...extra,
});
const screenAnswer = (extra: Record<string, unknown> = {}) => ({
  mode: 'screen',
  available: true,
  at: 1234,
  repaint: b64('\u001b[2Jhello'),
  size: { rows: 30, cols: 100 },
  history: { included: 10, omitted: 0, evicted: 0 },
  refused: {
    clipboard: 0,
    hyperlink: 0,
    window_op: 0,
    query: 0,
    oversized: 0,
    string: 0,
    invalid_utf8: 0,
  },
  parser: parser(),
  ...extra,
});

let state: WorkspaceState;
let calls: { command: string; request: Record<string, unknown> }[];
let attach: () => unknown;
let manager: TerminalSessionManager;
let restore: ReturnType<typeof vi.fn>;
let withRestore = true;

const renderer = (): TerminalRenderer => ({
  fit: vi.fn(() => ({ rows: 24, cols: 80 })),
  write: vi.fn(async () => true),
  input: vi.fn(() => vi.fn()),
  setInput: vi.fn(),
  setActive: vi.fn(),
  focus: vi.fn(),
  dispose: vi.fn(),
  ...(withRestore
    ? {
        restore: restore as unknown as NonNullable<TerminalRenderer['restore']>,
      }
    : {}),
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
  withRestore = true;
  restore = vi.fn(async () => true);
  attach = () => screenAnswer();
  state = { sessions: [], tabs: [], creating: false };
  mockIPC((command, args) => {
    const request =
      (args as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'host_terminals')
      return [{ terminal: info, title: 'T', created_at: 1, native: null }];
    if (command === 'terminal_attach') return attach();
    if (command === 'terminal_read_output') return new Promise(() => undefined);
    if (command === 'terminal_redraw') return null;
    return info;
  });
  manager = new TerminalSessionManager(
    (value) => {
      state = value;
    },
    (host, update, adopted, title, cwd, attachFirst) =>
      new TerminalController(
        host,
        renderer(),
        update,
        adopted,
        title,
        cwd,
        attachFirst,
      ),
    true,
  );
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const count = (c: string) => calls.filter((x) => x.command === c);
async function open() {
  manager.start();
  await flush();
  for (const s of state.sessions)
    manager.attach(s.id, document.createElement('div'));
  await flush();
}

it('draws the screen the host kept, at the size the program is at, before it reads on, and does not ask the program to redraw', async () => {
  await open();
  expect(count('terminal_attach')).toHaveLength(1);
  expect(restore).toHaveBeenCalledTimes(1);
  const [bytes, size] = restore.mock.calls[0]!;
  expect(new TextDecoder().decode(bytes as Uint8Array)).toBe('\u001b[2Jhello');
  expect(size).toEqual({ rows: 30, cols: 100 });
  expect(count('terminal_read_output')).toHaveLength(1);
  expect(count('terminal_redraw')).toHaveLength(0);
  expect(state.sessions[0]!.view.attach?.mode).toBe('screen');
  // Order: attach, then reading.
  const order = calls.map((c) => c.command);
  expect(order.indexOf('terminal_attach')).toBeLessThan(
    order.indexOf('terminal_read_output'),
  );
});

it('falls back to what it always did when the host kept no screen: replay and ask the program to draw again', async () => {
  attach = () => ({ mode: 'replay', reason: 'helper_missing', parser: null });
  await open();
  expect(restore).not.toHaveBeenCalled();
  expect(count('terminal_redraw')).toHaveLength(1);
  expect(state.sessions[0]!.view.attach).toMatchObject({
    mode: 'replay',
    reason: 'helper_missing',
  });
});

it('treats a host that fails or answers with something else as no screen, never as a broken terminal', async () => {
  for (const answer of [
    () => {
      throw { code: 'unknown_op' };
    },
    () => ({ mode: 'screen', repaint: 5 }),
    () => screenAnswer({ size: { rows: 0, cols: 80 } }),
    () => screenAnswer({ repaint: '***' }),
    () => screenAnswer({ parser: null }),
    () => null,
  ]) {
    calls = [];
    restore.mockClear();
    attach = answer;
    manager = new TerminalSessionManager(
      (value) => {
        state = value;
      },
      (host, update, adopted, title, cwd, attachFirst) =>
        new TerminalController(
          host,
          renderer(),
          update,
          adopted,
          title,
          cwd,
          attachFirst,
        ),
      true,
    );
    await open();
    expect(restore).not.toHaveBeenCalled();
    expect(state.sessions[0]!.view.state).toBe('running');
    expect(state.sessions[0]!.view.attach?.mode).toBe('replay');
    expect(count('terminal_redraw')).toHaveLength(1);
  }
});

it('does not even ask for a screen when the renderer cannot draw one', async () => {
  withRestore = false;
  await open();
  expect(count('terminal_attach')).toHaveLength(0);
  expect(count('terminal_redraw')).toHaveLength(1);
});

it('shows the parser health every read carries, without a call of its own', async () => {
  mockIPC((command, args) => {
    const request =
      (args as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'host_terminals')
      return [{ terminal: info, title: 'T', created_at: 1, native: null }];
    if (command === 'terminal_attach') return screenAnswer();
    if (command === 'terminal_read_output')
      return {
        batch_id: null,
        data: [],
        eof: false,
        truncated: false,
        status: info,
        parser: { health: 'restarting', fidelity: 'unavailable', restarts: 2 },
      };
    return info;
  });
  await open();
  await vi.advanceTimersByTimeAsync(50);
  expect(state.sessions[0]!.view.parser?.health).toBe('restarting');
  expect(count('terminal_parser')).toHaveLength(0);
});

// ---- the contract and the words --------------------------------------------------------------

it('decodes only exactly an attach answer', () => {
  const ok = parseAttach(screenAnswer());
  expect(ok.mode === 'screen' && ok.size).toEqual({ rows: 30, cols: 100 });
  expect(() => parseAttach(screenAnswer({ repaint: 'abc' }))).toThrow();
  expect(() =>
    parseAttach(screenAnswer({ refused: { clipboard: -1 } })),
  ).toThrow();
  expect(() =>
    parseAttach(screenAnswer({ size: { rows: 513, cols: 80 } })),
  ).toThrow();
  expect(() =>
    parseAttach(screenAnswer({ repaint: b64('x'.repeat(1024 * 1024 + 1)) })),
  ).toThrow();
  expect(parseAttach({ mode: 'replay', reason: 'x', parser: null })).toEqual({
    mode: 'replay',
    reason: 'x',
    parser: null,
  });
});

it('says what is missing and never "restored" alone when it is not whole', () => {
  const view = (attach: unknown, parserView?: unknown) =>
    ({ state: 'running', attach, parser: parserView }) as never;
  expect(parserNote(view(undefined))).toBeNull();
  expect(parserNote(view(parseAttach(screenAnswer())))).toBeNull();
  const tail = parserNote(
    view(
      parseAttach(
        screenAnswer({
          parser: parser({ fidelity: 'tail', missing_bytes: 3 * 1024 }),
        }),
      ),
    ),
  );
  expect(tail?.tone).toBe('warn');
  expect(tail?.text).toMatch(/3 KiB before it is not included/);
  const gap = parserNote(
    view(
      parseAttach(
        screenAnswer({
          parser: parser({ fidelity: 'gap', skipped_bytes: 5 * 1024 }),
        }),
      ),
    ),
  );
  expect(gap?.tone).toBe('warn');
  expect(gap?.text).toMatch(/5 KiB of output was missed/);
  expect(
    parserNote(
      view(undefined, { health: 'healthy', fidelity: 'gap', restarts: 0 }),
    )?.text,
  ).toMatch(/missed/);
  const replay = parserNote(
    view({ mode: 'replay', reason: 'helper_missing', parser: null }),
  );
  expect(replay?.text).toMatch(/no screen state was kept/);
  expect(replay?.title).toMatch(/helper missing/);
  expect(
    parserNote(
      view(undefined, { health: 'restarting', fidelity: 'tail', restarts: 1 }),
    )?.text,
  ).toMatch(/restarting/);
  expect(
    parserNote(
      view(undefined, {
        health: 'unavailable',
        fidelity: 'unavailable',
        restarts: 0,
      }),
    )?.text,
  ).toMatch(/unavailable/);
});

it('has no way for output to write the clipboard, open a link or run an application action', async () => {
  const { readFileSync } = await import('node:fs');
  const { join } = await import('node:path');
  const source = readFileSync(
    join(__dirname, '..', 'src/lib/features/terminal/renderer.ts'),
    'utf8',
  );
  // Only the fit add-on is loaded; no clipboard, link, image or serialize add-on exists.
  expect(
    [...source.matchAll(/from '@xterm\/([^']+)'/g)].map((m) => m[1]),
  ).toEqual(['xterm', 'addon-fit']);
  expect(source).not.toMatch(
    /registerOscHandler|registerCsiHandler|registerDcsHandler|registerEscHandler/,
  );
  expect(source).not.toMatch(
    /window\.open|navigator\.clipboard|location\.href/,
  );
  // A link in output is never activated.
  expect(source).toMatch(
    /linkHandler:\s*\{\s*activate:\s*\(\)\s*=>\s*undefined\s*\}/,
  );
  expect(source.match(/loadAddon\(/g)).toHaveLength(1);
});
