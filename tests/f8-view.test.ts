import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import fixture from './fixtures/f8-gui-v1.json';
import type { Raw } from './f8-harness';

const fakes = vi.hoisted(() => ({ factory: vi.fn() }));
vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: fakes.factory,
}));

const fx = fixture as Record<string, Raw>;
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));
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

interface Call {
  command: string;
  request: Record<string, Raw> | undefined;
}
interface Script {
  state?: () => unknown;
  sessions?: () => unknown;
  session?: () => unknown;
  turn?: (n: number) => unknown;
  approval?: () => unknown;
  decide?: (call: Call) => unknown;
  run?: (call: Call) => unknown;
}
/** A desktop whose terminal works and whose protocol-2 interface the test scripts. */
function desktop(script: Script = {}) {
  const calls: Call[] = [];
  let turns = 0;
  mockIPC((command, payload) => {
    const request = (payload as { request?: Record<string, Raw> } | undefined)
      ?.request;
    calls.push({ command, request });
    switch (command) {
      case 'f8_state':
        return (script.state ?? (() => fx.state))();
      case 'f8_reconnect':
        return fx.state;
      case 'f8_sessions':
        return (script.sessions ?? (() => fx.sessions))();
      case 'f8_session':
        return (script.session ?? (() => fx.session_attached))();
      case 'f8_turn_read':
        return (script.turn ?? ((n) => (n === 1 ? fx.turn_reading : never())))(
          ++turns,
        );
      case 'f8_approval_read':
        return (script.approval ?? (() => fx.approval))();
      case 'f8_approval_accept':
      case 'f8_approval_decline':
      case 'f8_approval_cancel':
        return (script.decide ?? (() => fx.decision_receipt))(calls.at(-1)!);
      case 'f8_audit':
        return fx.audit;
      case 'f8_audit_wait':
        return never();
      case 'f8_models':
        return fx.models;
      case 'authority_state':
      case 'terminal_read_output':
        return never();
      case 'get_app_info':
        return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
      default:
        if (command.startsWith('f8_'))
          return (script.run ?? (() => ({})))(calls.at(-1)!);
        return info;
    }
  });
  return {
    calls,
    count: (command: string) =>
      calls.filter((c) => c.command === command).length,
    last: (command: string) =>
      calls.filter((c) => c.command === command).at(-1),
  };
}
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label));
const view = () => document.querySelector<HTMLElement>('.runtimes-feature')!;
const button = (label: string, root: ParentNode = view()) =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  )!;
async function start(script: Script = {}) {
  const ipc = desktop(script);
  component = mount(App, { target: document.body });
  await flush();
  return ipc;
}
async function open(script: Script = {}) {
  const ipc = await start(script);
  nav('Runtimes')!.click();
  await flush();
  return ipc;
}
async function select(script: Script = {}) {
  const ipc = await open(script);
  view().querySelector<HTMLButtonElement>('.session-row')!.click();
  await flush();
  return ipc;
}

beforeEach(() => {
  // The fixture is a real run, and an approval waits five minutes: the clock is the
  // fixture's own, so the approval is as open as it was when the desktop sent it.
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(Date.parse(fx.approval.approval.requested_at) + 1000);
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
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('offers nothing of it on the installed desktop, which answers inactive', async () => {
  const ipc = await start({ state: () => fx.state_inactive });
  expect(nav('Runtimes')).toBeUndefined();
  expect(document.querySelector('.runtimes-feature')).toBeNull();
  expect(ipc.count('f8_sessions')).toBe(0);
  expect(ipc.count('f8_audit_wait')).toBe(0);
});

it('offers nothing of it when the desktop answers something that is not the projection', async () => {
  await start({ state: () => ({ active: true }) });
  expect(nav('Runtimes')).toBeUndefined();
});

it('shows Runtimes after Agents when the interface is active, and reads nothing before it is opened', async () => {
  const ipc = await start();
  const labels = Array.from(
    document.querySelectorAll('.sidebar .nav-label'),
  ).map((n) => n.textContent);
  expect(labels.indexOf('Runtimes')).toBe(labels.indexOf('Agents') + 1);
  expect(view().hidden).toBe(true);
  expect(view().inert).toBe(true);
  expect(ipc.count('f8_sessions')).toBe(0);
  nav('Runtimes')!.click();
  await flush();
  expect(view().hidden).toBe(false);
  expect(nav('Runtimes')!.getAttribute('aria-current')).toBe('page');
  expect(document.querySelector('.context-title')?.textContent).toBe(
    'Runtimes',
  );
  expect(ipc.last('f8_sessions')?.request).toEqual({ before: null });
  // The one wait for the next commit holds a watcher slot while the view is shown.
  expect(ipc.count('f8_audit_wait')).toBe(1);
  expect(text(view().querySelector('.sessions-count'))).toContain('protocol 2');
});

it('lists the sessions with the state of their runtime and opens one with its sections', async () => {
  await select();
  const row = view().querySelector('.session-row')!;
  expect(row.getAttribute('aria-pressed')).toBe('true');
  // The list is the page the authority sent: this session's runtime had not been started.
  expect(text(row)).toContain('Stopped');
  expect(text(view().querySelector('.rt-head .pill'))).toBe('Ready');
  const tabs = Array.from(view().querySelectorAll('[role="tab"]'));
  expect(tabs.map((t) => text(t))).toEqual([
    'Overview',
    'Conversation',
    'Models',
    'History',
  ]);
  expect(tabs[0].getAttribute('aria-selected')).toBe('true');
  expect(view().querySelector('[role="tabpanel"]')).not.toBeNull();
  expect(text(view().querySelector('.rt-next'))).toContain('Ready.');
  expect(
    text(view().querySelector('[data-testid="observed-model"]')),
  ).toContain('not reported');
});

it('reports Claude Managed as unavailable without offering a provider fallback', async () => {
  const ipc = await select();
  button('Overview').click();
  await flush();
  const availability = view().querySelector(
    '[data-testid="claude-managed-unavailable"]',
  );
  expect(availability).not.toBeNull();
  expect(text(availability)).toContain(
    'Claude Code Managed is not implemented',
  );
  expect(text(availability)).toContain('This session remains Codex');
  expect(text(availability)).toContain('separate, explicit launch');
  expect(button('Start Claude Code')).toBeUndefined();
  expect(ipc.count('f8_runtime_start')).toBe(0);
});

it('moves between sections with the arrow keys, as a tab list does', async () => {
  await select();
  const tabs = () =>
    Array.from(view().querySelectorAll<HTMLButtonElement>('[role="tab"]'));
  tabs()[0].focus();
  tabs()[0].dispatchEvent(
    new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }),
  );
  await flush();
  expect(tabs()[1].getAttribute('aria-selected')).toBe('true');
  expect(tabs()[1].tabIndex).toBe(0);
  expect(tabs()[0].tabIndex).toBe(-1);
  tabs()[1].dispatchEvent(
    new KeyboardEvent('keydown', { key: 'End', bubbles: true }),
  );
  await flush();
  expect(tabs()[3].getAttribute('aria-selected')).toBe('true');
  tabs()[3].dispatchEvent(
    new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }),
  );
  await flush();
  expect(tabs()[0].getAttribute('aria-selected')).toBe('true');
});

it('stops a runtime only after asking twice, with the fence and revision it was shown', async () => {
  const ipc = await select();
  button('Stop runtime').click();
  await flush();
  expect(ipc.count('f8_runtime_stop')).toBe(0);
  expect(view().querySelector('[aria-label="Confirm stop"]')).not.toBeNull();
  button('Keep running').click();
  await flush();
  expect(view().querySelector('[aria-label="Confirm stop"]')).toBeNull();
  button('Stop runtime').click();
  await flush();
  button('Stop runtime').click();
  await flush();
  const sent = ipc.last('f8_runtime_stop')!.request!;
  const runtime = fx.session_attached.session.runtime;
  expect(sent.fence).toEqual(runtime.fence);
  expect(sent.expected_revision).toBe(runtime.revision);
  expect(Object.keys(sent).sort()).toEqual([
    'expected_revision',
    'fence',
    'intent_id',
  ]);
});

it('offers a lease to grant only when none is active, with a closed set of scopes and lifetimes', async () => {
  const stopped = copy(fx.session_attached);
  stopped.lease = null;
  const ipc = await select({ session: () => stopped });
  const boxes = Array.from(
    view().querySelectorAll<HTMLInputElement>(
      '.rt-scopes input[type="checkbox"]',
    ),
  );
  expect(boxes).toHaveLength(5);
  expect(boxes.every((b) => b.checked)).toBe(true);
  boxes[4].click();
  await flush();
  const select_ = view().querySelector<HTMLSelectElement>('.rt-select select')!;
  expect(Array.from(select_.options).map((o) => o.textContent)).toEqual([
    '1 min',
    '5 min',
    '15 min',
    '30 min',
    '1 h',
  ]);
  select_.value = '900';
  select_.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  button('Grant lease').click();
  await flush();
  const sent = ipc.last('f8_lease_grant')!.request!;
  expect(sent.scopes).toEqual([
    'session_create',
    'session_resume',
    'turn_start',
    'command_execute',
  ]);
  expect(sent.ttl_seconds).toBe(900);
  expect(sent.fence).toEqual(stopped.session.runtime.fence);
});

it('shows a command waiting for approval whole, and sends each decision as its own command', async () => {
  const detail = copy(fx.session_attached);
  detail.turns = [copy(fx.turn_reading.turn)];
  detail.approval = copy(fx.approval);
  const ipc = await select({ session: () => detail });
  button('Conversation').click();
  await flush();
  const card = view().querySelector('.approval-card')!;
  expect(card).not.toBeNull();
  expect(text(card.querySelector('.approval-code'))).toBe(
    fx.approval.command.command,
  );
  expect(text(card.querySelector('.approval-fields'))).toContain(
    fx.approval.command.cwd,
  );
  for (const [label, command] of [
    ['Accept', 'f8_approval_accept'],
    ['Decline', 'f8_approval_decline'],
    ['Cancel action', 'f8_approval_cancel'],
  ] as const) {
    button(label, card).click();
    await flush();
    const sent = ipc.last(command)!.request!;
    expect(sent.fence).toEqual(fx.session_attached.session.runtime.fence);
    expect(sent.approval_id).toBe(fx.approval.approval.id);
    expect(sent.expected_revision).toBe(fx.approval.approval.revision);
    // A decision is the command, never a field of it.
    expect(Object.keys(sent).sort()).toEqual([
      'approval_id',
      'expected_revision',
      'fence',
      'intent_id',
    ]);
  }
});

it('offers only the decisions supported by the live native request', async () => {
  const detail = copy(fx.session_attached);
  detail.turns = [copy(fx.turn_reading.turn)];
  detail.approval = { ...copy(fx.approval), decisions: ['accept', 'cancel'] };
  await select({ session: () => detail, approval: () => detail.approval });
  button('Conversation').click();
  await flush();
  const card = view().querySelector('.approval-card')!;
  expect(card).not.toBeNull();
  expect(button('Accept', card)).toBeDefined();
  expect(button('Cancel action', card)).toBeDefined();
  expect(button('Decline', card)).toBeUndefined();
});

it('does not offer a decision when the approval cannot be decided any more', async () => {
  const detail = copy(fx.session_attached);
  detail.turns = [copy(fx.turn_reading.turn)];
  detail.approval = copy(fx.approval);
  const gone = copy(fx.approval);
  gone.approval.decidable = false;
  gone.command = null;
  const ipc = await select({ session: () => detail, approval: () => gone });
  button('Conversation').click();
  await flush();
  const card = view().querySelector('.approval-card')!;
  for (const label of ['Accept', 'Decline', 'Cancel action'])
    expect(button(label, card).disabled).toBe(true);
  expect(text(card)).toContain('no longer holds');
  button('Accept', card).click();
  await flush();
  expect(ipc.count('f8_approval_accept')).toBe(0);
});

it('says what a refused decision means and offers a retry only when nothing was refused', async () => {
  const detail = copy(fx.session_attached);
  detail.turns = [copy(fx.turn_reading.turn)];
  detail.approval = copy(fx.approval);
  let nth = 0;
  const ipc = await select({
    session: () => detail,
    decide: () => {
      throw nth++ === 0
        ? { code: 'unavailable', retry: true, retry_in_ms: null }
        : { code: 'permission_denied', retry: false, retry_in_ms: null };
    },
  });
  button('Conversation').click();
  await flush();
  button('Accept', view().querySelector('.approval-card')!).click();
  await flush();
  let alert = view().querySelector('.approval-card [role="alert"]')!;
  expect(text(alert)).toContain('cannot decide twice');
  button('Retry', alert).click();
  await flush();
  expect(ipc.count('f8_approval_accept')).toBe(2);
  const [first, second] = ipc.calls.filter(
    (c) => c.command === 'f8_approval_accept',
  );
  // Asked again with the identity it had, which is the same decision.
  expect(second.request).toEqual(first.request);
  alert = view().querySelector('.approval-card [role="alert"]')!;
  expect(text(alert)).toContain('denied');
  expect(
    Array.from(alert.querySelectorAll('button')).map((b) => text(b)),
  ).toEqual(['Dismiss']);
});

it('says the link is lost and offers no action while the authority does not answer', async () => {
  let failing = false;
  const ipc = await select({
    sessions: () => {
      if (failing)
        throw { code: 'unavailable', retry: true, retry_in_ms: 1000 };
      return fx.sessions;
    },
  });
  failing = true;
  button('Refresh').click();
  await flush();
  expect(view().textContent).toContain(
    'What is shown was read before the connection was lost',
  );
  expect(button('New session').disabled).toBe(true);
  expect(button('Stop runtime').disabled).toBe(true);
  button('Reconnect now').click();
  await flush();
  expect(ipc.count('f8_reconnect')).toBe(1);
});

it('names a replaced authority and shows nothing from before it', async () => {
  let replaced = false;
  const other = copy(fx.sessions);
  other.authority = '99999999-9999-4999-8999-999999999999';
  const stopped = copy(fx.session_stopped);
  stopped.authority = other.authority;
  const detail = copy(fx.session_attached);
  detail.turns = [copy(fx.turn_reading.turn)];
  detail.approval = copy(fx.approval);
  await select({
    sessions: () => (replaced ? other : fx.sessions),
    session: () => (replaced ? stopped : detail),
  });
  button('Conversation').click();
  await flush();
  expect(view().querySelector('.approval-card')).not.toBeNull();
  replaced = true;
  button('Refresh').click();
  await flush();
  const notice = view().querySelector('.sessions-status[role="alert"]')!;
  expect(text(notice)).toContain('restarted or was replaced');
  expect(view().querySelector('.approval-card')).toBeNull();
  button('Dismiss', notice).click();
  await flush();
  expect(view().textContent).not.toContain('restarted or was replaced');
});

it('shows the models the runtime offers with their efforts, and says a model is not selected here', async () => {
  await select();
  button('Models').click();
  await flush();
  const panel = view().querySelector('.rt-models')!;
  expect(text(panel)).toContain('Runtime default');
  expect(text(panel)).toContain('medium · default');
  expect(text(panel)).toContain('no request that selects a model');
  expect(panel.querySelectorAll('.rt-model').length).toBe(
    fx.models.entries.length,
  );
});

it('reads history with the newest first and narrows it to the session on request', async () => {
  await select();
  button('History').click();
  await flush();
  const panel = view().querySelector('.rt-history')!;
  expect(panel.querySelectorAll('.event-row').length).toBeGreaterThan(0);
  const own = panel.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  expect(own.checked).toBe(true);
  own.click();
  await flush();
  expect(text(panel)).toContain('Every session.');
});
