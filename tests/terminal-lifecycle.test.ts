import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import type { LandingPage } from '../src/lib/contracts/settings';
import { parseTerminalId } from '../src/lib/contracts/identities';
import {
  whenWorkspaceReady,
  registerWorkspace,
} from '../src/lib/features/native/bridge';
import { FakeDesktopSettings, withPreferences } from './settings-harness';

const fakes = vi.hoisted(() => ({ factory: vi.fn() }));
vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: fakes.factory,
}));

let component: ReturnType<typeof mount> | undefined;
let commands: string[] = [];
const info = {
  terminal_id: parseTerminalId('66666666-6666-4666-8666-666666666666'),
  shell: '/bin/fish',
  cwd: '/example',
  observed_pid: 321,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};
const never = () => new Promise(() => undefined);

/** Lets the mocked IPC answer and the component settle. */
async function settle() {
  for (let i = 0; i < 20; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
}

/** The desktop, with the preference the window opens on and the terminal commands recorded. */
function desktop(landing: LandingPage, failReads = false) {
  const settings = new FakeDesktopSettings(
    withPreferences((p) => ({
      ...p,
      interface: { ...p.interface, landing_page: landing },
    })),
  );
  settings.failReads = failReads;
  mockIPC((command, payload) => {
    commands.push(command);
    const request =
      (payload as { request?: Record<string, unknown> } | undefined)?.request ??
      {};
    const fromSettings = settings.ipc(command, request);
    if (fromSettings) return fromSettings;
    if (command === 'create_terminal') return info;
    if (command === 'host_terminals') return [];
    if (command === 'layout_read') return null;
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return never();
  });
}

const created = () => commands.filter((c) => c === 'create_terminal').length;

/** Clicks the sidebar item with this label. */
async function navigateTo(label: string) {
  const item = [...document.querySelectorAll('button')].find(
    (button) => button.textContent?.trim() === label,
  );
  if (!item) throw new Error(`no sidebar item "${label}"`);
  item.click();
  await settle();
}

beforeEach(() => {
  commands = [];
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
  await settle();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it('opens on Sessions without creating any shell', async () => {
  desktop('sessions');
  component = mount(App, { target: document.body });
  await settle();
  expect(document.body.textContent).toContain('Managed sessions');
  expect(commands).not.toContain('host_terminals');
  expect(created()).toBe(0);
});

it('opens on Console with exactly one terminal workspace', async () => {
  desktop('console');
  component = mount(App, { target: document.body });
  await settle();
  expect(commands.filter((c) => c === 'host_terminals')).toHaveLength(1);
  expect(created()).toBe(1);
});

it('opens the Console, with one terminal, when the preferences cannot be read', async () => {
  desktop('sessions', true);
  component = mount(App, { target: document.body });
  await settle();
  expect(created()).toBe(1);
});

it('keeps one terminal across Sessions, Console, Settings and back to Console', async () => {
  desktop('sessions');
  component = mount(App, { target: document.body });
  await settle();
  await navigateTo('Console');
  expect(created()).toBe(1);
  await navigateTo('Settings');
  await navigateTo('Console');
  expect(created()).toBe(1);
  expect(commands.filter((c) => c === 'host_terminals')).toHaveLength(1);
});

it('runs work queued before the workspace exists once it has its terminals', () => {
  const ran: string[] = [];
  whenWorkspaceReady((manager) => ran.push(`first:${typeof manager}`));
  whenWorkspaceReady(() => ran.push('second'));
  expect(ran).toEqual([]);
  const waiters: (() => void)[] = [];
  const manager = {
    whenReady(run: () => void) {
      waiters.push(run);
    },
  } as unknown as Parameters<typeof registerWorkspace>[0];
  const release = registerWorkspace(manager);
  expect(waiters).toHaveLength(2);
  expect(ran).toEqual([]);
  for (const run of waiters) run();
  expect(ran).toEqual(['first:object', 'second']);
  release();
});
