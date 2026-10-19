import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { LifecycleView } from '../src/lib/contracts/lifecycle';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import { listing } from './agent-harness';
import approvals from './fixtures/approval-list-v1.json';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v4.json';
import { lifecycle } from './lifecycle-harness';

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
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const connected = (revision = 3) => ({
  ...views.connected,
  revision,
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
});

interface Call {
  command: string;
  request: Record<string, unknown>;
}
type Answer<T> = (value: T) => void;
/**
 * A desktop whose terminal works. The authority link and the lifecycle are answered by
 * the test, one view at a time, the way the desktop answers its two followers.
 */
function desktop(link: unknown, first: LifecycleView) {
  const calls: Call[] = [];
  const world = {
    link: undefined as Answer<unknown> | undefined,
    lifecycle: undefined as Answer<LifecycleView> | undefined,
    /** What a Retry answers. Left waiting when it is a promise that does not settle. */
    retry: (): unknown => lifecycle({ phase: 'checking', revision: 90 }),
  };
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'authority_state') {
      if (request.after === null) return link;
      return new Promise((resolve) => (world.link = resolve));
    }
    if (command === 'authority_lifecycle') {
      if (request.after === null) return first;
      return new Promise<LifecycleView>(
        (resolve) => (world.lifecycle = resolve),
      );
    }
    if (command === 'authority_retry') return world.retry();
    if (command === 'managed_sessions_page') return pages.loaded;
    if (command === 'managed_turns') return listing([]);
    if (command === 'managed_approvals') return approvals.empty;
    if (command === 'managed_events')
      return {
        schema_version: 1,
        stream: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
        head: null,
        events: [],
        older: null,
        gap: false,
      };
    if (command === 'terminal_read_output') return never();
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return info;
  });
  const of = (command: string) => calls.filter((c) => c.command === command);
  const answer = async <T>(
    which: 'link' | 'lifecycle',
    value: T,
  ): Promise<void> => {
    for (let i = 0; i < 10 && !world[which]; i++) await flush();
    const resolve = world[which] as Answer<T>;
    world[which] = undefined;
    resolve(value);
    await flush();
  };
  return {
    calls,
    world,
    of,
    count: (command: string) => of(command).length,
    /** The desktop answers its lifecycle follower with another view. */
    says: (view: LifecycleView) => answer('lifecycle', view),
    /** The desktop answers its authority follower with another view. */
    links: (view: unknown) => answer('link', view),
  };
}
const pill = () =>
  text(document.querySelector('.status-cluster .pill.authority'));
const notice = () => document.querySelector<HTMLElement>('.authority-notice');
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label))!;
const button = (label: string, root: ParentNode = document) =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  );
async function click(target: HTMLElement | undefined) {
  expect(target).toBeDefined();
  target!.click();
  await flush();
}
async function launched(link: unknown, first: LifecycleView) {
  const ipc = desktop(link, first);
  component = mount(App, { target: document.body });
  await flush();
  return ipc;
}

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
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.innerHTML = '';
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it('shows the authority starting by itself, with the terminal working meanwhile', async () => {
  const ipc = await launched(
    views.waiting_absent,
    lifecycle({ phase: 'installing', revision: 2 }),
  );
  // Nobody asked for anything: the desktop said it is installing, and that is shown.
  expect(pill()).toBe('Authority Installing');
  expect(notice()).toBeNull();
  expect(ipc.count('authority_retry')).toBe(0);
  // The terminal is there and was started. It waits on nothing of this.
  expect(ipc.count('create_terminal')).toBe(1);
  expect(
    document.querySelector('.terminal-feature')!.hasAttribute('hidden'),
  ).toBe(false);
  expect(text(document.querySelector('.pill.terminal'))).toBe(
    'Terminal Running',
  );

  await ipc.says(lifecycle({ phase: 'starting', revision: 3 }));
  expect(pill()).toBe('Authority Starting');
  await ipc.says(lifecycle({ phase: 'connecting', revision: 4 }));
  expect(pill()).toBe('Authority Starting');
  // The lifecycle says ready before the link has connected. That is not connected.
  await ipc.says(lifecycle({ phase: 'ready', origin: 'managed', revision: 5 }));
  expect(pill()).not.toContain('Connected');
  expect(pill()).toBe('Authority Offline');
  // The link connects: now it is.
  await ipc.links(connected());
  expect(pill()).toContain('Authority Connected');
  expect(notice()).toBeNull();
  expect(ipc.count('authority_retry')).toBe(0);
  expect(ipc.count('create_terminal')).toBe(1);
});

it('says when the authority could not start and retries once when asked', async () => {
  const ipc = await launched(
    views.waiting_absent,
    lifecycle({
      phase: 'failed',
      failure: 'exited_repeatedly',
      revision: 2,
      diagnostics: {
        build: 'packaged',
        installation: 'current',
        service: 'failed',
      },
    }),
  );
  expect(pill()).toBe('Authority Failed to start');
  expect(text(notice()!.querySelector('.authority-notice-title'))).toBe(
    'The Wyss authority could not start',
  );
  expect(text(notice()!.querySelector('.authority-notice-text'))).toBe(
    'The authority ended repeatedly and is no longer being restarted.',
  );
  // The details are words. No command, unit, path or process is on the page.
  expect(
    Array.from(notice()!.querySelectorAll('dl > *')).map((node) => text(node)),
  ).toEqual([
    'This build',
    'Packaged: carries its own authority',
    'Installation',
    'Installed',
    'Service',
    'Failed',
  ]);
  expect(text(document.body)).not.toMatch(
    /systemctl|systemd|\.service|\bpid\b|\.sock|journal|sudo/i,
  );
  // It is shown above whatever view is open, and takes none away.
  expect(
    document.querySelector('.terminal-feature')!.hasAttribute('hidden'),
  ).toBe(false);
  expect(ipc.count('authority_retry')).toBe(0);

  // Retry: one request that names nothing, however often it is pressed.
  let answerRetry!: (view: LifecycleView) => void;
  ipc.world.retry = () =>
    new Promise<LifecycleView>((resolve) => (answerRetry = resolve));
  await click(button('Retry', notice()!));
  expect(button('Retrying…', notice()!)!.disabled).toBe(true);
  button('Retrying…', notice()!)!.click();
  await flush();
  expect(ipc.of('authority_retry')).toEqual([
    { command: 'authority_retry', request: {} },
  ]);
  answerRetry(lifecycle({ phase: 'checking', revision: 3 }));
  await flush();
  // What comes of it arrives through the follower, not through the answer.
  await ipc.says(lifecycle({ phase: 'starting', revision: 4 }));
  expect(notice()).toBeNull();
  expect(pill()).toBe('Authority Starting');
  await ipc.says(
    lifecycle({ phase: 'failed', failure: 'startup_failed', revision: 5 }),
  );
  expect(text(notice()!.querySelector('.authority-notice-text'))).toBe(
    'The authority was started and ended before it answered.',
  );
  expect(button('Retry', notice()!)!.disabled).toBe(false);
  // A Retry the bridge refuses leaves Retry usable.
  ipc.world.retry = () => Promise.reject({ code: 'busy' });
  await click(button('Retry', notice()!));
  expect(button('Retry', notice()!)!.disabled).toBe(false);
  expect(ipc.count('authority_retry')).toBe(2);
});

it('says an incompatible authority is running and offers nothing against it', async () => {
  await launched(
    { ...views.waiting_absent, reason: 'incompatible' },
    lifecycle({ phase: 'incompatible', origin: 'external', revision: 2 }),
  );
  expect(pill()).toBe('Authority Incompatible');
  expect(text(notice()!.querySelector('.authority-notice-title'))).toBe(
    'An incompatible Wyss authority is running',
  );
  expect(text(notice())).toContain('left exactly as it is');
  // Nothing to press: it is not this installation's to stop, replace or restart.
  expect(
    Array.from(notice()!.querySelectorAll('button')).map((b) => text(b)),
  ).toEqual([]);
});

it('keeps one lifecycle for every view, and no view starts anything for it', async () => {
  const ipc = await launched(
    views.waiting_absent,
    lifecycle({ phase: 'failed', failure: 'stopped', revision: 2 }),
  );
  expect(pill()).toBe('Authority Offline');
  expect(text(notice()!.querySelector('.authority-notice-title'))).toBe(
    'The Wyss authority is not running',
  );
  const reads = ipc.count('authority_lifecycle');
  // Every view is opened in turn. Each shows the same notice; none asks for the
  // lifecycle again, and none retries by itself.
  for (const view of ['Sessions', 'Agents', 'Approvals', 'Events', 'Console']) {
    await click(nav(view));
    expect(document.querySelector('.context-title')?.textContent).toBe(view);
    expect(document.querySelectorAll('.authority-notice')).toHaveLength(1);
  }
  expect(ipc.count('authority_lifecycle')).toBe(reads);
  expect(ipc.count('authority_retry')).toBe(0);
  // One follower: one read waiting, the one after the view it was given.
  expect(ipc.of('authority_lifecycle').map((c) => c.request.after)).toEqual([
    null,
    2,
  ]);
  // It comes back, started by whoever: the notice goes with it.
  await ipc.links(connected());
  expect(notice()).toBeNull();
  await ipc.says(
    lifecycle({
      phase: 'ready',
      origin: 'managed',
      update_pending: true,
      revision: 3,
    }),
  );
  expect(pill()).toContain('Authority Connected update pending');
  expect(
    document.querySelector('.pill.authority')!.getAttribute('title'),
  ).toContain('a newer authority is installed the next time it starts');
  expect(notice()).toBeNull();
});

it('goes on without a lifecycle the bridge cannot answer', async () => {
  const ipc = desktop(views.waiting_absent, lifecycle());
  // An answer that is not the view: the shell says what the link says, and no more.
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    ipc.calls.push({ command, request });
    if (command === 'authority_state')
      return request.after === null ? views.waiting_absent : never();
    if (command === 'authority_lifecycle') return { phase: 'ready', pid: 1 };
    if (command === 'terminal_read_output') return never();
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return info;
  });
  component = mount(App, { target: document.body });
  await flush();
  expect(pill()).toBe('Authority Offline');
  expect(notice()).toBeNull();
  expect(ipc.count('create_terminal')).toBe(1);
});
