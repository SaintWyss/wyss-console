import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import { EventsController } from '../src/lib/features/events/controller';
import { parseTerminalId } from '../src/lib/contracts/identities';
import type { LifecycleView } from '../src/lib/contracts/lifecycle';
import {
  currentAppearance,
  resetAppearance,
} from '../src/lib/features/terminal/appearance';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import views from './fixtures/authority-view-v1.json';
import { lifecycle } from './lifecycle-harness';
import { FakeDesktopSettings, withPreferences } from './settings-harness';

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

/** A desktop whose terminal works, whose preferences are `settings`, and whose authority is as told. */
function desktop(
  settings: FakeDesktopSettings,
  link: unknown = connected,
  first: LifecycleView = lifecycle({ phase: 'ready', origin: 'managed' }),
) {
  const commands: string[] = [];
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    commands.push(command);
    if (command === 'authority_state')
      return request.after === null ? link : never();
    if (command === 'authority_lifecycle')
      return request.after === null ? first : never();
    if (command === 'authority_retry') return first;
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '0.2.0-test' };
    if (command === 'terminal_read_output') return never();
    const answered = settings.ipc(command, request);
    if (answered) return answered;
    if (command.startsWith('managed_')) return never();
    return info;
  });
  return commands;
}
const openSettings = async () => {
  const item = Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes('Settings'))!;
  item.click();
  await flush();
};
const page = () => document.querySelector<HTMLElement>('.settings-feature')!;
const select = (id: string) =>
  document.querySelector<HTMLSelectElement>(`#${id}`)!;
async function choose(id: string, value: string) {
  const control = select(id);
  control.value = value;
  control.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
}
const button = (label: string | RegExp) =>
  Array.from(page().querySelectorAll('button')).find((b) =>
    typeof label === 'string' ? text(b) === label : label.test(text(b)),
  );
async function show(
  settings = new FakeDesktopSettings(),
  link: unknown = connected,
  first?: LifecycleView,
) {
  const commands = desktop(settings, link, first);
  component = mount(App, { target: document.body });
  await flush();
  await openSettings();
  return commands;
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
  await flush();
  resetAppearance();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it('opens from the sidebar with every section, in order, and the defaults selected', async () => {
  await show();
  expect(text(document.querySelector('.context-title'))).toBe('Settings');
  expect(Array.from(page().querySelectorAll('h3')).map((h) => text(h))).toEqual(
    [
      'Appearance',
      'Interface',
      'Terminal',
      'Events',
      'Authority',
      'About',
      'Reset',
      'Projects and workspaces',
    ],
  );
  expect(select('setting-landing').value).toBe('console');
  expect(select('setting-font').value).toBe('16');
  expect(select('setting-scrollback').value).toBe('5000');
  expect(select('setting-cursor').value).toBe('block');
  expect(
    document.querySelector<HTMLInputElement>('#setting-blink')!.checked,
  ).toBe(false);
  expect(select('setting-time').value).toBe('system');
  expect(select('setting-filter').value).toBe('all');
  expect(text(page().querySelector('.sessions-count'))).toBe(
    'Saved on this computer',
  );
});

it('does not pretend to have a light theme', async () => {
  await show();
  const section = page().querySelector(
    '[aria-labelledby="settings-appearance"]',
  )!;
  expect(text(section)).toContain('dark theme');
  expect(section.querySelectorAll('select, input, button')).toHaveLength(0);
});

it('offers exactly seven preferences, none of them free text', async () => {
  await show();
  const controls = Array.from(
    page().querySelectorAll<HTMLElement>('select, input, textarea'),
  );
  expect(controls.map((c) => c.id)).toEqual([
    'setting-landing',
    'setting-font',
    'setting-scrollback',
    'setting-cursor',
    'setting-blink',
    'setting-time',
    'setting-filter',
  ]);
  expect(controls.filter((c) => c.tagName === 'TEXTAREA')).toHaveLength(0);
  for (const input of page().querySelectorAll<HTMLInputElement>('input'))
    expect(input.type).toBe('checkbox');
  // Every control has a label of its own.
  for (const control of controls)
    expect(
      page().querySelector(`label[for="${control.id}"]`),
      control.id,
    ).not.toBeNull();
});

it('exposes nothing that would weaken a boundary', async () => {
  await show();
  const wording = [
    ...page().querySelectorAll('label'),
    ...page().querySelectorAll('option'),
    ...page().querySelectorAll('button'),
  ]
    .map((n) => text(n))
    .join(' | ');
  for (const forbidden of [
    /auto.?approv/i,
    /always/i,
    /remember/i,
    /trust/i,
    /local tools/i,
    /default (agent )?mode/i,
    /inference/i,
    /sandbox/i,
    /isolation/i,
    /workspace/i,
    /shell/i,
    /command/i,
    /environment/i,
    /executable/i,
    /socket/i,
    /service/i,
    /audit/i,
    /retention/i,
    /permission/i,
    /policy/i,
    /bypass/i,
  ])
    expect(wording, String(forbidden)).not.toMatch(forbidden);
  // And the page says what preferences never do.
  expect(text(page().querySelector('.settings-lede'))).toContain(
    'sessions start in Inference',
  );
  expect(text(page().querySelector('.settings-lede'))).toContain(
    'waits for your approval',
  );
});

it('saves each preference as one whole typed set, and shows it kept', async () => {
  const settings = new FakeDesktopSettings();
  await show(settings);
  const changes: [
    string,
    string,
    (p: typeof settings.preferences) => unknown,
    unknown,
  ][] = [
    ['setting-landing', 'events', (p) => p.interface.landing_page, 'events'],
    ['setting-font', '20', (p) => p.terminal.font_size, 20],
    ['setting-scrollback', '20000', (p) => p.terminal.scrollback, 20000],
    [
      'setting-cursor',
      'underline',
      (p) => p.terminal.cursor_style,
      'underline',
    ],
    ['setting-time', 'h24', (p) => p.events.time_format, 'h24'],
    [
      'setting-filter',
      'approvals',
      (p) => p.events.default_filter,
      'approvals',
    ],
  ];
  for (const [id, value, read, expected] of changes) {
    await choose(id, value);
    expect(read(settings.preferences), id).toEqual(expected);
    expect(select(id).value).toBe(value);
  }
  const blink = document.querySelector<HTMLInputElement>('#setting-blink')!;
  blink.checked = true;
  blink.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  expect(settings.preferences.terminal.cursor_blink).toBe(true);
  // Each write was the whole set, with the schema version, and nothing else.
  for (const call of settings.calls.filter((c) => c.command === 'update'))
    expect(Object.keys(call.sent as object).sort()).toEqual([
      'events',
      'interface',
      'schema_version',
      'terminal',
    ]);
  expect(settings.calls.filter((c) => c.command === 'update')).toHaveLength(7);
});

it('shows what was saved last time, and applies it to the terminals', async () => {
  const settings = new FakeDesktopSettings(
    withPreferences((p) => ({
      ...p,
      terminal: {
        font_size: 20,
        scrollback: 10_000,
        cursor_style: 'bar',
        cursor_blink: true,
      },
      events: { time_format: 'h12', default_filter: 'security' },
    })),
  );
  await show(settings);
  expect(select('setting-font').value).toBe('20');
  expect(select('setting-scrollback').value).toBe('10000');
  expect(select('setting-cursor').value).toBe('bar');
  expect(select('setting-time').value).toBe('h12');
  expect(select('setting-filter').value).toBe('security');
  expect(currentAppearance()).toEqual({
    fontSize: 20,
    scrollback: 10_000,
    cursorStyle: 'bar',
    cursorBlink: true,
  });
  await choose('setting-font', '12');
  expect(currentAppearance().fontSize).toBe(12);
});

it('keeps a scrollback it was handed that is not one of the steps', async () => {
  const settings = new FakeDesktopSettings(
    withPreferences((p) => ({
      ...p,
      terminal: { ...p.terminal, scrollback: 12_345 },
    })),
  );
  await show(settings);
  expect(select('setting-scrollback').value).toBe('12345');
  expect(text(select('setting-scrollback'))).toContain('12,345 lines');
});

it('opens on the preferred view, unless the user already went somewhere', async () => {
  const settings = new FakeDesktopSettings(
    withPreferences((p) => ({ ...p, interface: { landing_page: 'events' } })),
  );
  desktop(settings);
  component = mount(App, { target: document.body });
  await flush();
  expect(text(document.querySelector('.context-title'))).toBe('Events');
  await unmount(component);
  component = undefined;
  document.body.replaceChildren();

  // The answer comes late and the user has already moved: they are not taken away.
  const late = new FakeDesktopSettings(
    withPreferences((p) => ({ ...p, interface: { landing_page: 'events' } })),
  );
  let answer!: (value: unknown) => void;
  const inner = late.read.bind(late);
  late.read = () =>
    new Promise((resolve) => {
      answer = () => resolve(inner());
    });
  desktop(late);
  component = mount(App, { target: document.body });
  await flush();
  document
    .querySelectorAll<HTMLButtonElement>('.sidebar .nav-item')[2]!
    .click();
  await flush();
  answer(undefined);
  await flush();
  expect(text(document.querySelector('.context-title'))).toBe('Sessions');
});

it('starts the Events view on the preferred filter, and follows a change of it', async () => {
  const spy = vi.spyOn(EventsController.prototype, 'setFilter');
  const settings = new FakeDesktopSettings(
    withPreferences((p) => ({
      ...p,
      events: { time_format: 'system', default_filter: 'approvals' },
    })),
  );
  await show(settings);
  expect(spy).toHaveBeenLastCalledWith('approvals');
  await choose('setting-filter', 'security');
  expect(spy).toHaveBeenLastCalledWith('security');
});

it('opens on the Console when the preferences cannot be read', async () => {
  const settings = new FakeDesktopSettings();
  settings.failReads = true;
  desktop(settings);
  component = mount(App, { target: document.body });
  await flush();
  expect(text(document.querySelector('.context-title'))).toBe('Console');
  await openSettings();
  expect(page().querySelector('[role="alert"]')?.textContent).toContain(
    'could not be read',
  );
  // Nothing can be changed or reset on a guess.
  for (const control of page().querySelectorAll<HTMLSelectElement>('select'))
    expect(control.disabled).toBe(true);
  expect((button(/Reset preferences/) as HTMLButtonElement).disabled).toBe(
    true,
  );
  settings.failReads = false;
  (button('Try again') as HTMLButtonElement).click();
  await flush();
  expect(page().querySelector('[role="alert"]')).toBeNull();
  expect(select('setting-font').disabled).toBe(false);
});

it('says so when a change was not kept, and shows the old value again', async () => {
  const settings = new FakeDesktopSettings();
  await show(settings);
  settings.failNext = 'not_saved';
  await choose('setting-font', '22');
  expect(select('setting-font').value).toBe('16');
  expect(text(page().querySelector('[role="alert"]'))).toBe(
    'The preference could not be saved. Nothing was changed.',
  );
  // No raw error text can reach the page.
  expect(text(page())).not.toMatch(/not_saved|internal|Error:/);
});

it('explains each way the saved preferences were not used', async () => {
  for (const [storage, title] of [
    ['recovered', 'could not be read'],
    ['unsafe', 'were not trusted'],
    ['newer', 'newer version'],
    ['unavailable', 'cannot be saved here'],
  ] as const) {
    const settings = new FakeDesktopSettings(undefined, storage);
    await show(settings);
    const notice = page().querySelector('.settings-notice')!;
    expect(text(notice), storage).toContain(title);
    expect(notice.getAttribute('data-tone'), storage).toMatch(/warn|error/);
    // Whatever the reason, no path or file name is shown.
    expect(text(page()), storage).not.toMatch(
      /preferences\.json|\.config|\/home/,
    );
    await unmount(component!);
    component = undefined;
    document.body.replaceChildren();
  }
});

it('cannot change preferences that a newer version saved, but can reset them', async () => {
  const settings = new FakeDesktopSettings(undefined, 'newer');
  await show(settings);
  for (const control of page().querySelectorAll<HTMLSelectElement>('select'))
    expect(control.disabled).toBe(true);
  (button(/Reset preferences/) as HTMLButtonElement).click();
  await flush();
  (button('Reset preferences') as HTMLButtonElement).click();
  await flush();
  expect(settings.storage).toBe('ready');
  expect(page().querySelector('.settings-notice')).toBeNull();
  expect(select('setting-font').disabled).toBe(false);
});

it('asks before resetting, says what it does not touch, and resets only preferences', async () => {
  const settings = new FakeDesktopSettings(
    withPreferences((p) => ({
      ...p,
      terminal: { ...p.terminal, font_size: 22 },
    })),
  );
  const commands = await show(settings);
  (button('Reset preferences…') as HTMLButtonElement).click();
  await flush();
  const dialog = page().querySelector('[role="alertdialog"]')!;
  expect(text(dialog)).toContain('Reset all preferences to their defaults?');
  for (const kept of [
    'sessions',
    'history',
    'approvals',
    'workspaces',
    'Codex login',
  ])
    expect(text(dialog)).toContain(kept);
  // Nothing happened yet, and everything else is out of reach while it is asked.
  expect(settings.calls.map((c) => c.command)).not.toContain('reset');
  expect(select('setting-font').disabled).toBe(true);
  (button('Cancel') as HTMLButtonElement).click();
  await flush();
  expect(page().querySelector('[role="alertdialog"]')).toBeNull();
  expect(settings.preferences.terminal.font_size).toBe(22);

  (button('Reset preferences…') as HTMLButtonElement).click();
  await flush();
  (button('Reset preferences') as HTMLButtonElement).click();
  await flush();
  expect(settings.calls.filter((c) => c.command === 'reset')).toHaveLength(1);
  expect(select('setting-font').value).toBe('16');
  expect(currentAppearance().fontSize).toBe(16);
  // The reset touched the preferences and nothing the authority owns: no session, event,
  // approval or lifecycle command was sent for it.
  expect(
    commands.filter((c) => /^managed_/.test(c) || c === 'authority_retry'),
  ).toEqual([]);
});

it('brings the reset confirmation into view and gives the safe answer the focus', async () => {
  // jsdom lays nothing out: what is checked here is that the confirmation asks to be
  // shown and that Cancel holds the focus. That it then fits a 960×640 window was
  // observed in a real browser engine.
  const scrolled: Element[] = [];
  const original = Element.prototype.scrollIntoView;
  Element.prototype.scrollIntoView = function (this: Element) {
    scrolled.push(this);
  };
  try {
    const settings = new FakeDesktopSettings();
    await show(settings);
    (button('Reset preferences…') as HTMLButtonElement).click();
    await flush();
    const dialog = page().querySelector('[role="alertdialog"]')!;
    expect(scrolled).toContain(dialog);
    expect(document.activeElement).toBe(button('Cancel'));
    // Enter on what has the focus resets nothing.
    (document.activeElement as HTMLButtonElement).click();
    await flush();
    expect(settings.calls.map((c) => c.command)).not.toContain('reset');
  } finally {
    Element.prototype.scrollIntoView = original;
  }
});

it('does not touch the authority when a preference changes', async () => {
  const settings = new FakeDesktopSettings();
  const commands = await show(settings);
  const before = commands.length;
  await choose('setting-font', '18');
  await choose('setting-landing', 'sessions');
  expect(commands.slice(before)).toEqual([
    'settings_update',
    'settings_update',
  ]);
});

it('reads the preferences once and never polls them', async () => {
  const settings = new FakeDesktopSettings();
  vi.useFakeTimers({ toFake: ['setTimeout', 'setInterval'] });
  try {
    await show(settings);
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(settings.calls.filter((c) => c.command === 'read')).toHaveLength(1);
  } finally {
    vi.useRealTimers();
  }
});

// ---------------------------------------------------------------- authority and about

const facts = () =>
  Object.fromEntries(
    Array.from(
      page().querySelectorAll('[aria-labelledby="settings-authority"] dt'),
    ).map((dt) => [text(dt), text(dt.nextElementSibling)]),
  );

it('shows a connected authority in the product’s words, read-only', async () => {
  await show();
  expect(facts()).toEqual({
    Authority: 'Connected',
    'Codex runtime': 'Ready',
    Installation: 'Managed by Wyss Console',
    Compatibility: 'Compatible',
    'Authority version': '9.9.9-test',
    Protocol: 'Version 1',
  });
  // Read only: the one action the lifecycle offers is Retry, and nothing failed.
  expect(button('Retry')).toBeUndefined();
});

it('says an authority started outside is just that, and that an update waits', async () => {
  await show(
    undefined,
    connected,
    lifecycle({ phase: 'ready', origin: 'external' }),
  );
  expect(facts().Installation).toBe('Started outside Wyss Console');
  await unmount(component!);
  component = undefined;
  document.body.replaceChildren();
  await show(
    undefined,
    connected,
    lifecycle({ phase: 'ready', origin: 'managed', update_pending: true }),
  );
  expect(facts().Update).toBe(
    'A newer authority is installed the next time it starts',
  );
});

it('says an authority that is not running is not running, and that terminals do not care', async () => {
  await show(
    undefined,
    views.waiting_absent,
    lifecycle({
      phase: 'failed',
      failure: 'stopped',
      diagnostics: {
        build: 'packaged',
        installation: 'current',
        service: 'inactive',
      },
    }),
  );
  expect(facts().Authority).toBe('Not running');
  expect(facts()['Codex runtime']).toBe('Not available while offline');
  expect(facts().Installation).toBe('Installed by Wyss Console');
  expect(facts().Compatibility).toBe('Checked when the authority answers');
  expect(facts()['Authority version']).toBeUndefined();
  expect(text(page())).toContain(
    'Local terminals and these preferences work without it',
  );
  // Retry is offered, and it is the lifecycle's own Retry.
  expect(button('Retry')).toBeDefined();
});

it('asks for a Retry through the one lifecycle intent', async () => {
  const commands = await show(
    undefined,
    views.waiting_absent,
    lifecycle({ phase: 'failed', failure: 'startup_failed' }),
  );
  (button('Retry') as HTMLButtonElement).click();
  await flush();
  expect(commands.filter((c) => c === 'authority_retry')).toHaveLength(1);
});

it('says an incompatible authority is not compatible with this app', async () => {
  await show(
    undefined,
    { ...views.waiting_absent, reason: 'incompatible' },
    lifecycle({ phase: 'incompatible', origin: 'external' }),
  );
  expect(facts().Compatibility).toBe(
    'This Wyss authority is not compatible with this version of the app.',
  );
});

it('shows nothing of the system underneath, whatever the authority is doing', async () => {
  const states: [unknown, LifecycleView][] = [
    [connected, lifecycle({ phase: 'ready', origin: 'managed' })],
    [
      views.waiting_absent,
      lifecycle({ phase: 'failed', failure: 'manager_unavailable' }),
    ],
    [views.waiting_absent, lifecycle({ phase: 'installing' })],
    [
      views.waiting_absent,
      lifecycle({ phase: 'incompatible', origin: 'managed' }),
    ],
  ];
  for (const [link, first] of states) {
    await show(undefined, link, first);
    expect(text(page())).not.toMatch(
      /\/run|\/home|\/usr|\.sock|\.service|systemctl|systemd|wyss-control|unix|pid|sha-?256|digest|credential|auth\.json|token|raw/i,
    );
    await unmount(component!);
    component = undefined;
    document.body.replaceChildren();
  }
});

it('lets the page be read while the authority link is still being made', async () => {
  await show(undefined, never(), lifecycle({ phase: 'checking' }));
  expect(facts().Authority).toBe('Connecting');
  expect(facts()['Codex runtime']).toBe('Not available while offline');
});

it('has an About with the version, the build, the platform and the protocol', async () => {
  await show();
  const about = Object.fromEntries(
    Array.from(
      page().querySelectorAll('[aria-labelledby="settings-about"] dt'),
    ).map((dt) => [text(dt), text(dt.nextElementSibling)]),
  );
  expect(about).toEqual({
    'Wyss Console': 'Version 0.2.0-test',
    Build: '0123456789ab',
    Platform: 'Linux (x86_64)',
    'Authority protocol': 'Version 1',
  });
});

it('leaves out the build and the protocol when they are not known', async () => {
  const settings = new FakeDesktopSettings();
  const view = settings.view.bind(settings);
  settings.view = () => ({
    ...view(),
    about: { version: '1.2.3', revision: null, platform: 'linux-aarch64' },
  });
  await show(settings, views.waiting_absent, lifecycle({ phase: 'checking' }));
  const terms = Array.from(
    page().querySelectorAll('[aria-labelledby="settings-about"] dt'),
  ).map((dt) => text(dt));
  expect(terms).toEqual(['Wyss Console', 'Platform']);
});

it('believes no view the desktop gives that is not a view', async () => {
  const settings = new FakeDesktopSettings();
  desktop(settings);
  // Re-answer the read with something wrong: the page falls back, it does not guess.
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    if (command === 'settings_read')
      return { ...settings.view(), storage: 'ready; rm -rf /' };
    if (command === 'authority_state')
      return request.after === null ? connected : never();
    if (command === 'authority_lifecycle')
      return request.after === null ? lifecycle({ phase: 'ready' }) : never();
    if (command === 'terminal_read_output') return never();
    if (command.startsWith('managed_')) return never();
    return info;
  });
  component = mount(App, { target: document.body });
  await flush();
  await openSettings();
  expect(text(page().querySelector('[role="alert"]'))).toContain(
    'could not be read',
  );
  expect(select('setting-font').disabled).toBe(true);
  expect(text(page())).not.toContain('rm -rf');
});
