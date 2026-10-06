import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mount, unmount, flushSync } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import TerminalView from '../src/lib/features/terminal/TerminalView.svelte';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';

const fakes = vi.hoisted(() => ({ factory: vi.fn() }));
vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: fakes.factory,
}));
let component: ReturnType<typeof mount> | undefined;
let renders: TerminalRenderer[];
let commands: string[];
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
class Observer {
  observe() {}
  disconnect() {}
}
const flush = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
  flushSync();
};
beforeEach(() => {
  commands = [];
  renders = [];
  vi.stubGlobal('ResizeObserver', Observer);
  fakes.factory.mockImplementation(() => {
    const render: TerminalRenderer = {
      fit: () => info.size,
      write: async () => true,
      input: () => () => undefined,
      setInput: vi.fn(),
      focus: vi.fn(),
      dispose: vi.fn(),
    };
    renders.push(render);
    return render;
  });
  mockIPC((command) => {
    commands.push(command);
    return command === 'terminal_read_output'
      ? new Promise(() => undefined)
      : command === 'get_app_info'
        ? { schema_version: 1, name: 'Wyss Console', version: '1B-test' }
        : info;
  });
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  await flush();
  vi.unstubAllGlobals();
});
it('mounts one PTY, renders running state, closes once and disposes renderer', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(1);
  expect(document.querySelector('[role=status]')?.textContent).toContain(
    '/bin/fish',
  );
  document.querySelector('button')!.click();
  await flush();
  expect(document.querySelector('[role=status]')?.textContent).toContain(
    'Terminal closed',
  );
  expect(commands.filter((c) => c === 'close_terminal')).toHaveLength(1);
  expect(renders[0].dispose).toHaveBeenCalledTimes(1);
});
it('shows natural exit, retains the renderer and does not respawn', async () => {
  mockIPC((command) => {
    commands.push(command);
    return command === 'terminal_read_output'
      ? {
          batch_id: null,
          data: [],
          eof: true,
          truncated: false,
          status: { ...info, state: 'exited', exit: { code: 0, signal: null } },
        }
      : info;
  });
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(document.querySelector('[role=status]')?.textContent).toContain(
    'Shell exited (0)',
  );
  expect(renders[0].dispose).not.toHaveBeenCalled();
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(1);
});
it('shows terminal-local creation failure without internal details', async () => {
  mockIPC(() =>
    Promise.reject({ code: 'spawn_failed', diagnostic: 'private chain' }),
  );
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(document.body.textContent).toContain('Terminal failed: spawn_failed');
  expect(document.body.textContent).not.toContain('private chain');
});
it('serializes remount retirement, preventing leaked duplicate ownership', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  await unmount(component);
  component = undefined;
  await flush();
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(2);
  expect(commands.indexOf('close_terminal')).toBeLessThan(
    commands.lastIndexOf('create_terminal'),
  );
});
it('keeps AppInfo IPC in the minimal application header', async () => {
  component = mount(App, { target: document.body });
  await flush();
  expect(document.querySelector('h1')?.textContent).toBe('Wyss Console');
  expect(document.querySelector('header')?.textContent).toContain('1B-test');
  expect(commands.filter((c) => c === 'get_app_info')).toHaveLength(1);
});

it('reports renderer initialization failure without crashing the app', async () => {
  fakes.factory.mockImplementationOnce(() => {
    throw new Error('private renderer detail');
  });
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(document.body.textContent).toContain(
    'Terminal failed: renderer_error',
  );
  expect(document.body.textContent).not.toContain('private renderer detail');
  expect(commands).not.toContain('create_terminal');
});

it('preserves narrow IPC capabilities and production script/network restrictions', async () => {
  const { default: config } = await import('../src-tauri/tauri.conf.json');
  const { default: capability } =
    await import('../src-tauri/capabilities/main.json');
  expect(capability.permissions).toEqual([
    'allow-get-app-info',
    'allow-create-terminal',
    'allow-terminal-write',
    'allow-terminal-read-output',
    'allow-terminal-resize',
    'allow-close-terminal',
    'allow-terminal-status',
  ]);
  const directives = Object.fromEntries(
    config.app.security.csp
      .split(';')
      .filter((v) => v.trim())
      .map((v) => {
        const [name, ...values] = v.trim().split(' ');
        return [name, values];
      }),
  );
  expect(directives['default-src']).toEqual(["'self'"]);
  expect(directives['connect-src']).toEqual(['ipc:', 'http://ipc.localhost']);
  expect(directives['style-src']).toEqual(["'self'"]);
  expect(directives['style-src-elem']).toEqual(["'self'", "'unsafe-inline'"]);
  expect(directives['style-src-attr']).toEqual(["'unsafe-inline'"]);
  expect(config.app.security.csp).not.toContain('unsafe-eval');
});
