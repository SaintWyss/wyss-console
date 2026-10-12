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
  static instances: Observer[] = [];
  target?: Element;
  constructor(readonly callback: () => void) {
    Observer.instances.push(this);
  }
  observe(target: Element) {
    this.target = target;
  }
  disconnect = vi.fn();
}
const flush = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
  flushSync();
};
beforeEach(() => {
  commands = [];
  renders = [];
  Observer.instances = [];
  vi.stubGlobal('ResizeObserver', Observer);
  fakes.factory.mockImplementation(() => {
    const render: TerminalRenderer = {
      fit: () => info.size,
      write: async () => true,
      input: () => () => undefined,
      setInput: vi.fn(),
      setActive: vi.fn(),
      focus: vi.fn(),
      dispose: vi.fn(),
    };
    renders.push(render);
    return render;
  });
  let ordinal = 0;
  mockIPC((command) => {
    commands.push(command);
    if (command === 'create_terminal') {
      ordinal++;
      return {
        ...info,
        terminal_id: parseTerminalId(
          `00000000-0000-4000-8000-${String(ordinal).padStart(12, '0')}`,
        ),
      };
    }
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
  vi.restoreAllMocks();
});
it('mounts one PTY, renders running state, closes once and disposes renderer', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(1);
  expect(document.querySelector('[role=status]')?.textContent).toContain(
    '/bin/fish',
  );
  document
    .querySelector<HTMLButtonElement>('[aria-label="Close Terminal 1"]')!
    .click();
  await flush();
  expect(document.body.textContent).toContain('No terminals open');
  expect(document.querySelectorAll('[role=tab]')).toHaveLength(0);
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
  // The authority is reached through one read of the desktop's own projection, and
  // managed sessions through one read and six intents, managed turns through two reads
  // and two intents, their approvals through two reads and two decisions, and the
  // authority's durable history through two reads. Nothing takes a method name.
  // A workspace is chosen through one native dialog: the window is granted no
  // filesystem, shell, process, socket, dialog or event permission of the framework.
  expect(capability.permissions).toEqual([
    'allow-get-app-info',
    'allow-authority-state',
    'allow-authority-lifecycle',
    'allow-authority-retry',
    'allow-managed-sessions-page',
    'allow-managed-session-prepare',
    'allow-managed-session-resume',
    'allow-managed-session-close',
    'allow-managed-session-activate',
    'allow-managed-session-abandon',
    'allow-managed-session-mode',
    'allow-choose-workspace',
    'allow-managed-turns',
    'allow-managed-turn-read',
    'allow-managed-turn-start',
    'allow-managed-turn-cancel',
    'allow-managed-approvals',
    'allow-managed-approval-read',
    'allow-managed-approval-approve',
    'allow-managed-approval-deny',
    'allow-managed-events',
    'allow-managed-events-older',
    'allow-settings-read',
    'allow-settings-update',
    'allow-settings-reset',
    'allow-create-terminal',
    'allow-terminal-write',
    'allow-terminal-read-output',
    'allow-terminal-resize',
    'allow-close-terminal',
    'allow-terminal-status',
  ]);
  // Every permission is one of this application's own commands: none belongs to a
  // plugin (`fs:`, `shell:`, `dialog:`) or to the framework's core (`core:`).
  for (const permission of capability.permissions)
    expect(permission).toMatch(/^allow-[a-z-]+$/);
  // An approval is answered by exactly two commands, one decision each. Nothing the
  // window is granted makes a rule, remembers a choice, edits a policy, changes a
  // lease, names a runtime profile or passes a control request through.
  expect(
    capability.permissions.filter((p: string) => /approv/.test(p)),
  ).toEqual([
    'allow-managed-approvals',
    'allow-managed-approval-read',
    'allow-managed-approval-approve',
    'allow-managed-approval-deny',
  ]);
  // History is read by exactly two commands. Nothing reads the audit as the authority
  // stores it, watches it, exports it, or performs again what an event describes.
  expect(
    capability.permissions.filter((p: string) => /event|audit|histor/.test(p)),
  ).toEqual(['allow-managed-events', 'allow-managed-events-older']);
  // The desktop's own preferences are one read, one replacement of the whole typed set
  // and one reset. There is no key to write, no file to name and nothing to run.
  expect(
    capability.permissions.filter((p: string) => /setting/.test(p)),
  ).toEqual([
    'allow-settings-read',
    'allow-settings-update',
    'allow-settings-reset',
  ]);
  // The window is granted none of the framework's own plugins or core commands: every
  // permission is an application command, so no generic file, shell or process access
  // came with the settings.
  expect(
    capability.permissions.filter(
      (p: string) =>
        p.includes(':') || /^allow-(fs|shell|process|opener|os|http)/.test(p),
    ),
  ).toEqual([]);
  // The authority's lifecycle is one read and Retry. Nothing the window is granted
  // starts, stops or restarts a service by name, runs a command, installs a file at a
  // path, or talks to the service manager.
  expect(
    capability.permissions.filter((p: string) =>
      /authority|lifecycle|retry/.test(p),
    ),
  ).toEqual([
    'allow-authority-state',
    'allow-authority-lifecycle',
    'allow-authority-retry',
  ]);
  for (const permission of capability.permissions)
    expect(permission).not.toMatch(
      /always|remember|trust|rule|policy|allowlist|respond|decision|lease|scope|profile|runtime|control|request|invoke|exec|shell|spawn|process|socket|fs|audit|watch|replay|export|save|payload|envelope|service|unit|systemd|systemctl|dbus|install|binary|path|restart|stop|kill|command/,
    );
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

const newTerminal = () =>
  Array.from(document.querySelectorAll('button')).find(
    (b) => b.textContent === 'New Terminal',
  )!;
it('creates distinct tabs, ARIA panels and selects the new terminal', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  newTerminal().click();
  await flush();
  const tabs = Array.from(document.querySelectorAll('[role=tab]'));
  expect(tabs).toHaveLength(2);
  expect(tabs[0].getAttribute('aria-selected')).toBe('false');
  expect(tabs[1].getAttribute('aria-selected')).toBe('true');
  expect(document.querySelectorAll('[role=tabpanel]')).toHaveLength(2);
  for (const tab of tabs)
    expect(
      document
        .getElementById(tab.getAttribute('aria-controls')!)
        ?.getAttribute('aria-labelledby'),
    ).toBe(tab.id);
});
it('switches visible panels without remounting either renderer', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  newTerminal().click();
  await flush();
  const tabs = Array.from(
    document.querySelectorAll<HTMLButtonElement>('[role=tab]'),
  );
  tabs[0].click();
  await flush();
  const panels = Array.from(
    document.querySelectorAll<HTMLElement>('[role=tabpanel]'),
  );
  expect(panels[0].hidden).toBe(false);
  expect(panels[1].hidden).toBe(true);
  expect(panels[1].inert).toBe(true);
  expect(renders).toHaveLength(2);
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(2);
  expect(renders[0].dispose).not.toHaveBeenCalled();
  expect(renders[1].dispose).not.toHaveBeenCalled();
});
it('closes the exact inactive tab, keeps active session and shows final empty state', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  newTerminal().click();
  await flush();
  document
    .querySelector<HTMLButtonElement>('[aria-label="Close Terminal 1"]')!
    .click();
  await flush();
  expect(document.querySelector('[role=tab]')?.textContent).toContain(
    'Terminal 2',
  );
  expect(
    document.querySelector('[role=tab]')?.getAttribute('aria-selected'),
  ).toBe('true');
  document
    .querySelector<HTMLButtonElement>('[aria-label="Close Terminal 2"]')!
    .click();
  await flush();
  expect(document.body.textContent).toContain('No terminals open');
  expect(newTerminal().disabled).toBe(false);
  newTerminal().click();
  await flush();
  expect(document.querySelector('[role=tab]')?.textContent).toContain(
    'Terminal 3',
  );
});
it('rapid new-terminal clicks admit one creation at a time', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  newTerminal().click();
  newTerminal().click();
  await flush();
  expect(document.querySelectorAll('[role=tab]')).toHaveLength(2);
  expect(commands.filter((c) => c === 'create_terminal')).toHaveLength(2);
});
it('moves focus within tablist without intercepting terminal arrows or auto-selecting', async () => {
  component = mount(TerminalView, { target: document.body });
  await flush();
  newTerminal().click();
  await flush();
  const tabs = Array.from(
    document.querySelectorAll<HTMLButtonElement>('[role=tab]'),
  );
  tabs[0].focus();
  tabs[0].dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'ArrowRight',
      bubbles: true,
      cancelable: true,
    }),
  );
  expect(document.activeElement).toBe(tabs[1]);
  expect(tabs[1].getAttribute('aria-selected')).toBe('true');
  tabs[1].dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'Home',
      bubbles: true,
      cancelable: true,
    }),
  );
  expect(document.activeElement).toBe(tabs[0]);
  expect(tabs[0].getAttribute('aria-selected')).toBe('false');
  tabs[0].click();
  await flush();
  expect(tabs[0].getAttribute('aria-selected')).toBe('true');
  const arrow = new KeyboardEvent('keydown', {
    key: 'ArrowRight',
    bubbles: true,
    cancelable: true,
  });
  document.querySelector('.terminal-host')!.dispatchEvent(arrow);
  expect(arrow.defaultPrevented).toBe(false);
});
it('renders exited lifecycle on the retained tab', async () => {
  mockIPC((command) =>
    command === 'terminal_read_output'
      ? {
          batch_id: null,
          data: [],
          eof: true,
          truncated: false,
          status: { ...info, state: 'exited', exit: { code: 0, signal: null } },
        }
      : info,
  );
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(document.querySelector('[role=tab]')?.textContent).toContain('exited');
  expect(document.querySelectorAll('[role=tabpanel]')).toHaveLength(1);
});
it('shows isolated failed tab and keeps new-terminal action available', async () => {
  mockIPC(() => Promise.reject({ code: 'spawn_failed' }));
  component = mount(TerminalView, { target: document.body });
  await flush();
  expect(document.querySelector('[role=tab]')?.textContent).toContain('failed');
  expect(newTerminal().disabled).toBe(false);
  expect(
    document.querySelector<HTMLButtonElement>('[aria-label="Close Terminal 1"]')
      ?.disabled,
  ).toBe(false);
});

it('keeps the active tab and its close control visible on selection and strip resize', async () => {
  let width = 200;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
    function (this: HTMLElement) {
      if (this.classList.contains('terminal-tabs'))
        return new DOMRect(0, 0, width, 44);
      if (this.classList.contains('terminal-tab')) {
        const strip = this.parentElement!;
        const index = Array.from(strip.children).indexOf(this);
        return new DOMRect(index * 140 - strip.scrollLeft, 0, 140, 44);
      }
      return new DOMRect();
    },
  );
  component = mount(TerminalView, { target: document.body });
  await flush();
  newTerminal().click();
  await flush();
  const strip = document.querySelector<HTMLElement>('.terminal-tabs')!;
  expect(strip.scrollLeft).toBe(80);
  width = 160;
  Observer.instances.find((o) => o.target === strip)!.callback();
  expect(strip.scrollLeft).toBe(120);
  document.querySelector<HTMLButtonElement>('[role=tab]')!.click();
  await flush();
  expect(strip.scrollLeft).toBe(0);
  await unmount(component!);
  component = undefined;
  await flush();
  for (const observer of Observer.instances)
    expect(observer.disconnect).toHaveBeenCalledTimes(1);
});
