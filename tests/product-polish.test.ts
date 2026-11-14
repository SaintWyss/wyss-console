import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import { parseTerminalId } from '../src/lib/contracts/identities';
import type { LifecycleView } from '../src/lib/contracts/lifecycle';
import { resetAppearance } from '../src/lib/features/terminal/appearance';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { listing } from './agent-harness';
import approvals from './fixtures/approval-list-v1.json';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v4.json';
import { lifecycle } from './lifecycle-harness';
import { rawPage } from './events-harness';
import { FakeDesktopSettings } from './settings-harness';

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
  for (let i = 0; i < 40; i++) await Promise.resolve();
  flushSync();
};
const never = () => new Promise(() => undefined);
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const connected = {
  ...views.connected,
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
};

function desktop(link: unknown, first: LifecycleView) {
  const settings = new FakeDesktopSettings();
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    if (command === 'authority_state')
      return request.after === null ? link : never();
    if (command === 'authority_lifecycle')
      return request.after === null ? first : never();
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '0.2.0' };
    if (command === 'terminal_read_output') return never();
    if (command === 'managed_sessions_page') return pages.empty;
    if (command === 'managed_turns') return listing([]);
    if (command === 'managed_approvals') return approvals.empty;
    if (command === 'managed_events') return rawPage(0, 0);
    return settings.ipc(command, request) ?? info;
  });
}
const views_ = [
  'Console',
  'Sessions',
  'Agents',
  'Approvals',
  'Events',
  'Settings',
];
async function visit(name: string) {
  const item = Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => text(b).startsWith(name))!;
  item.click();
  await flush();
}
const visible = () =>
  Array.from(
    document.querySelectorAll<HTMLElement>(
      '.content > section[class$="-feature"]:not([hidden])',
    ),
  );

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
  await flush();
  resetAppearance();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

// What a person who never saw the code must not read in the product.
const ROUGH = [
  /\bTODO\b/i,
  /\bFIXME\b/i,
  /placeholder/i,
  /\bdebug/i,
  /fixture/i,
  /\bsprint\b/i,
  /\bphase \d/i,
  /\bADR\b/,
  /coming soon/i,
  /\bundefined\b/,
  /\[object /,
  /\bNaN\b/,
  /\bnull\b/,
  /wyss-control/,
  /\b[a-z]+(?:_[a-z]+)+\b/, // a raw enum or error code
  /::|Error:|panicked|stack trace/i,
  /RuntimeSafetyProfile|CodexLocalToolsV1|codex_local_tools|Control Protocol/,
];

for (const [label, link, first] of [
  ['connected', connected, lifecycle({ phase: 'ready', origin: 'managed' })],
  [
    'offline',
    views.waiting_absent,
    lifecycle({ phase: 'failed', failure: 'stopped' }),
  ],
  ['starting', views.waiting_absent, lifecycle({ phase: 'starting' })],
  [
    'incompatible',
    { ...views.waiting_absent, reason: 'incompatible' },
    lifecycle({ phase: 'incompatible', origin: 'external' }),
  ],
] as const) {
  it(`shows no development wording on any view while ${label}`, async () => {
    desktop(link, first);
    component = mount(App, { target: document.body });
    await flush();
    for (const name of views_) {
      await visit(name);
      expect(visible(), name).toHaveLength(1);
      const shown = text(document.querySelector('.shell'));
      // The controls and their option lists are words too; the dev-only test hooks
      // (attributes) are not text, so textContent is what a person reads.
      for (const rough of ROUGH)
        expect(shown, `${label}/${name}: ${rough}`).not.toMatch(rough);
    }
  });
}

it('names every view once, with a useful empty state and a way back to the Console', async () => {
  desktop(connected, lifecycle({ phase: 'ready', origin: 'managed' }));
  component = mount(App, { target: document.body });
  await flush();
  const expected: Record<string, RegExp> = {
    Sessions: /No managed sessions yet\./,
    Agents: /No session is open\./,
    Approvals: /Nothing is waiting for a decision\./,
    Events: /No events yet\./,
  };
  // The heading a view draws itself; null where it draws none and the shell titles it.
  const headings: Record<string, string | null> = {
    Sessions: 'Managed sessions',
    Agents: null,
    Approvals: 'Approvals',
    Events: 'Events',
    Settings: 'Settings',
  };
  const named = (name: string) => {
    const title = document.querySelector('.context-title');
    const own = headings[name];
    if (own === null) expect(text(title), name).toBe(name);
    else {
      // Never both: the name is not said twice.
      expect(title, name).toBeNull();
      expect(text(visible()[0].querySelector('h2')), name).toBe(own);
    }
  };
  for (const [name, empty] of Object.entries(expected)) {
    await visit(name);
    named(name);
    expect(text(visible()[0]), name).toMatch(empty);
  }
  await visit('Settings');
  named('Settings');
  // The Console never left: its terminal is still the one the workspace opened.
  await visit('Console');
  expect(text(document.querySelector('.pill.terminal'))).toContain('Running');
});

it('says on every managed view that being offline does not break the terminal', async () => {
  desktop(
    views.waiting_absent,
    lifecycle({ phase: 'failed', failure: 'stopped' }),
  );
  component = mount(App, { target: document.body });
  await flush();
  // The terminal stays Running while the authority is not.
  expect(text(document.querySelector('.pill.terminal'))).toContain('Running');
  for (const name of ['Sessions', 'Approvals', 'Events']) {
    await visit(name);
    expect(text(visible()[0]), name).toMatch(/The authority is unavailable/);
    expect(text(visible()[0]), name).toMatch(
      /(terminals|Console) (keep|keeps) working|do not depend on it/,
    );
  }
  await visit('Settings');
  expect(text(visible()[0])).toContain(
    'Local terminals and these preferences work without it',
  );
});
