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
  expect(document.querySelector('.sidebar')?.textContent).toContain('1B-test');
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
    'allow-managed-model-catalogue',
    'allow-managed-session-prepare',
    'allow-managed-session-resume',
    'allow-managed-session-close',
    'allow-managed-session-activate',
    'allow-managed-session-abandon',
    'allow-managed-session-mode',
    'allow-choose-workspace',
    'allow-managed-turns',
    'allow-managed-turns-older',
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
    // The agents' configuration: two reads of what the desktop already holds.
    'allow-configuration-inventory',
    'allow-configuration-changes',
    // Changing it, with a way back: a plan, its confirmation, the history, an undo, the
    // hooks that were turned off, and their export and import in native windows.
    'allow-configuration-plan',
    'allow-configuration-apply',
    'allow-configuration-history',
    'allow-configuration-undo',
    'allow-configuration-records',
    'allow-configuration-record-delete',
    'allow-configuration-export',
    'allow-configuration-import',
    'allow-configuration-author-hook',
    'allow-configuration-edit-hook',
    'allow-configuration-author-mcp',
    'allow-configuration-edit-mcp',
    'allow-configuration-edit-document',
    'allow-configuration-document-export',
    'allow-configuration-document-import',
    'allow-configuration-create-document',
    'allow-configuration-project',
    'allow-configuration-select-project',
    'allow-configuration-template-export',
    // Arsenal (AM1a): the offline library. Reads, one import from a fresh scan, restore, presets and
    // selections, and the configuration preview. None starts a session or writes a provider's files.
    'allow-arsenal-discover',
    'allow-arsenal-candidate',
    'allow-arsenal-capabilities',
    'allow-arsenal-capability',
    'allow-arsenal-file',
    'allow-arsenal-import',
    'allow-arsenal-restore',
    'allow-arsenal-presets',
    'allow-arsenal-preset-write',
    'allow-arsenal-preset-delete',
    'allow-arsenal-selections',
    'allow-arsenal-selection-write',
    'allow-arsenal-selection-delete',
    'allow-arsenal-preview',
    'allow-arsenal-terminal-plan',
    'allow-arsenal-terminal-bind',
    'allow-arsenal-terminal-bindings',
    'allow-arsenal-project-selection',
    'allow-arsenal-sync-status',
    'allow-arsenal-sync-now',
    // Native agent sessions (F3): the closed catalogue, a start that names an agent, a
    // project the user chose and a profile, the registry, and the profiles. None takes a
    // command, an argument list, an environment or a path.
    'allow-native-launcher',
    'allow-native-readiness',
    'allow-native-preview',
    'allow-native-start',
    'allow-native-sessions',
    'allow-native-stop',
    'allow-native-restart',
    'allow-native-forget',
    'allow-native-choose-project',
    'allow-native-project-default',
    'allow-native-profiles',
    'allow-native-profile-create',
    'allow-native-profile-save',
    'allow-native-profile-delete',
    'allow-native-profile-duplicate',
    'allow-native-profile-restore',
    'allow-native-profile-export',
    'allow-native-profile-import',
    'allow-launch-list',
    'allow-launch-get',
    'allow-launch-before',
    // The host that holds the terminals past the window (F9): what it holds, whether it is
    // a host process, and a redraw of a program's screen after a window attaches.
    'allow-host-terminals',
    'allow-host-info',
    'allow-terminal-redraw',
    // M4: the screen a terminal kept while the window was away, to draw before reading on.
    'allow-terminal-attach',
    'allow-terminal-attach-commit',
    'allow-host-persistence',
    'allow-host-lingering',
    // The layout of tabs and panes (T2): one read, one bounded write of a JSON object.
    'allow-layout-read',
    'allow-layout-write',
    // API credentials (F14): a key is typed in a native window and never returned.
    'allow-credentials-list',
    'allow-credential-add',
    'allow-credential-replace',
    'allow-credential-rename',
    'allow-credential-delete',
    'allow-credential-verify',
    // A handoff brief (F11a): prepared for review, and saved to a chosen file.
    'allow-native-handoff-prepare',
    'allow-native-handoff-save',
    // Delegated agents: the list of the tasks sessions handed to other agents, and a cancel.
    'allow-native-delegations',
    'allow-native-delegation-cancel',
    // Whether the protocol-2 interface exists (F8). The rest of it is the `f8` capability,
    // which the installed configuration does not enable.
    'allow-f8-state',
  ]);
  // Every permission is one of this application's own commands: none belongs to a
  // plugin (`fs:`, `shell:`, `dialog:`) or to the framework's core (`core:`).
  for (const permission of capability.permissions)
    expect(permission).toMatch(/^allow-[a-z0-9-]+$/);
  // Turns are read by exactly three commands (the newest, the ones before them, and one
  // turn's stream) and changed by exactly two (start and cancel). Reading older turns
  // takes a session and a position it was handed: nothing that starts, cancels or decides.
  expect(capability.permissions.filter((p: string) => /turn/.test(p))).toEqual([
    'allow-managed-turns',
    'allow-managed-turns-older',
    'allow-managed-turn-read',
    'allow-managed-turn-start',
    'allow-managed-turn-cancel',
  ]);
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
  // (The change history of the configuration is the desktop's own journal of what the user
  // changed, read by one command of its own: it is not the authority's audit.)
  expect(
    capability.permissions.filter(
      (p: string) =>
        /event|audit|histor/.test(p) && !p.startsWith('allow-configuration-'),
    ),
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
  // One exception, deliberate and narrow (ADR 0034, F2a): exporting a kept hook. The page
  // names a record it was given and nothing else; the passphrase and the file are asked
  // for in native windows the page cannot reach, and what is written is ciphertext.
  // The instruction-template export has the same native chooser/passphrase boundary:
  // only an inventory ID enters; ciphertext leaves through the chosen native file.
  // The hook and MCP-server template export (ADR 0050) is the same: only an inventory ID
  // enters, the passphrase and the file are asked in native windows, ciphertext leaves.
  // Second exception, deliberate and bounded (ADR 0035, F3): the native sessions' own
  // commands. They name an agent of a closed catalogue, a project the native dialog
  // returned, a session or a profile of the desktop's own registry, and so they are
  // allowed words like stop, restart, profile and export; the words that would mean the
  // window can run something of its own stay forbidden for them below.
  for (const permission of capability.permissions.filter(
    (p: string) =>
      p !== 'allow-configuration-export' &&
      p !== 'allow-configuration-document-export' &&
      p !== 'allow-configuration-template-export' &&
      !p.startsWith('allow-native-'),
  ))
    expect(permission).not.toMatch(
      /always|remember|trust|rule|policy|allowlist|respond|decision|lease|scope|profile|runtime|control|request|invoke|exec|shell|spawn|process|socket|fs|audit|watch|replay|export|save|payload|envelope|service|unit|systemd|systemctl|dbus|install|binary|path|restart|stop|kill|command/,
    );
  for (const permission of capability.permissions.filter((p: string) =>
    p.startsWith('allow-native-'),
  ))
    expect(permission).not.toMatch(
      /exec|shell|spawn|command|path|invoke|socket|systemd|systemctl|dbus|install|binary|process|fs|run|env|argv|argument/,
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

/** New terminal is the first entry of the Launch menu: opens the menu when it is closed. */
const newTerminal = () => {
  const find = () =>
    Array.from(document.querySelectorAll('.launch-panel button')).find((b) =>
      b.textContent?.includes('New terminal'),
    ) as HTMLButtonElement | undefined;
  if (!find()) {
    flushSync(() =>
      document.querySelector<HTMLButtonElement>('.launch-trigger')!.click(),
    );
  }
  return find()!;
};
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
