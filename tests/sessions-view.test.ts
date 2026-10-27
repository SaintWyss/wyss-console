import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v4.json';

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
const [fresh, bound, closed] = pages.loaded.sessions;
const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const flush = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
  flushSync();
};
const never = () => new Promise(() => undefined);
const FOLDER = '/home/dev/project';
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const ready = {
  ...views.connected,
  authority: { ...views.connected.authority, last_sequence: '9' },
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
};

interface Call {
  command: string;
  request: Record<string, unknown>;
}
/** A desktop whose terminal works and whose authority and sessions the test scripts. */
function desktop(script: {
  authority?: (after: number | null) => unknown;
  page?: (before: string | null, read: number) => unknown;
  intent?: (call: Call) => unknown;
  /** One step of an activation; `step` counts them from one. */
  activate?: (call: Call, step: number) => unknown;
  /** One step of a change of mode; `step` counts them from one. */
  mode?: (call: Call, step: number) => unknown;
  choose?: () => unknown;
}) {
  const calls: Call[] = [];
  mockIPC((command, payload) => {
    const request = (payload as { request?: Record<string, unknown> })?.request;
    calls.push({ command, request: request ?? {} });
    if (command === 'authority_state') {
      const after = request!.after as number | null;
      return (script.authority ?? ((a) => (a === null ? ready : never())))(
        after,
      );
    }
    if (command === 'managed_sessions_page')
      return (script.page ?? (() => pages.loaded))(
        request!.before as string | null,
        calls.filter((c) => c.command === command).length,
      );
    if (command === 'choose_workspace')
      return (script.choose ?? (() => ({ path: FOLDER })))();
    if (command === 'managed_session_activate')
      return (script.activate ?? (() => ({ stage: 'ready' })))(
        calls.at(-1)!,
        calls.filter((c) => c.command === command).length,
      );
    if (command === 'managed_session_mode')
      return (script.mode ?? (() => ({ stage: 'ready' })))(
        calls.at(-1)!,
        calls.filter((c) => c.command === command).length,
      );
    if (command.startsWith('managed_session_'))
      return (script.intent ?? (() => ({ id: fresh.id })))(calls.at(-1)!);
    if (command === 'terminal_read_output') return never();
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return info;
  });
  return {
    calls,
    count: (command: string) =>
      calls.filter((c) => c.command === command).length,
    sent: (prefix = 'managed_session_') =>
      calls.filter((c) => c.command.startsWith(prefix)),
  };
}
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label))!;
const view = () => document.querySelector<HTMLElement>('.sessions-feature')!;
const button = (label: string, root: ParentNode = view()) =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  )!;
const rows = () =>
  Array.from(view().querySelectorAll<HTMLButtonElement>('.session-row'));
const details = () => view().querySelector<HTMLElement>('.session-details');
async function open(label = 'Sessions') {
  nav(label).click();
  await flush();
}
async function start(script: Parameters<typeof desktop>[0] = {}) {
  const ipc = desktop(script);
  component = mount(App, { target: document.body });
  await flush();
  await open();
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
  await flush();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it('opens Sessions from the sidebar and reads nothing before that', async () => {
  const ipc = desktop({});
  component = mount(App, { target: document.body });
  await flush();
  expect(nav('Sessions').disabled).toBe(false);
  expect(view().hidden).toBe(true);
  expect(view().inert).toBe(true);
  // On the Console, sessions are not read at all.
  expect(ipc.count('managed_sessions_page')).toBe(0);

  await open();
  expect(nav('Sessions').getAttribute('aria-current')).toBe('page');
  expect(nav('Console').getAttribute('aria-current')).toBeNull();
  expect(document.querySelector('.context-title')?.textContent).toBe(
    'Sessions',
  );
  expect(document.querySelector('.context-subtitle')?.textContent).toBe(
    'Managed agent sessions',
  );
  expect(view().hidden).toBe(false);
  expect(ipc.sent('managed_sessions_page')).toEqual([
    { command: 'managed_sessions_page', request: { before: null } },
  ]);
  // The counts are the authority's own, from the one projection the shell follows.
  expect(view().querySelector('.sessions-count')?.textContent).toContain(
    '1 prepared · 0 closed',
  );
});

it('lists the sessions the authority holds, newest first', async () => {
  await start();
  const list = rows();
  expect(list).toHaveLength(3);
  expect(
    list.map((r) => r.querySelector('.session-name')?.textContent),
  ).toEqual(['Session 33333333', 'rag-corp', 'wyss']);
  expect(
    list.map((r) => r.querySelector('.pill')?.textContent?.trim()),
  ).toEqual(['Prepared', 'Prepared', 'Closed']);
  expect(list[0].textContent).toContain('Codex');
  expect(list[0].textContent).toContain('No workspace');
  expect(list[1].textContent).toContain('/home/dev/rag-corp');
  expect(list[1].textContent).toContain('Detached');
  expect(list[2].querySelector('.pill')?.getAttribute('data-tone')).toBe(
    'idle',
  );
  expect(list[1].querySelector('time')?.getAttribute('datetime')).toBe(
    bound.updated_at,
  );
  // Nothing is selected, so there is no details panel and no pager for one page.
  expect(details()).toBeNull();
  expect(view().querySelector('.sessions-pager')).toBeNull();
});

it('shows a useful empty state for an authority with no sessions', async () => {
  await start({ page: () => pages.empty });
  expect(rows()).toHaveLength(0);
  const empty = view().querySelector('.sessions-empty')!;
  expect(empty.textContent).toContain('No managed sessions yet.');
  expect(text(empty)).toContain(
    'Create a session to let Wyss supervise Codex in one of your projects.',
  );
  expect(button('New session', empty).disabled).toBe(false);
});

it('says it is reading instead of showing an endless nothing', async () => {
  await start({ page: never });
  const state = view().querySelector('.sessions-empty')!;
  expect(state.textContent).toContain('Reading sessions…');
  expect(state.getAttribute('aria-busy')).toBe('true');
  expect(button('Refresh').disabled).toBe(true);
});

it('shows a failed read as a failure, with a way to read again', async () => {
  const ipc = await start({
    page: (_, read) =>
      read === 1 ? Promise.reject({ code: 'busy', retry: true }) : pages.loaded,
  });
  const alert = view().querySelector('[role="alert"]')!;
  expect(alert.textContent).toContain('Sessions could not be read.');
  expect(rows()).toHaveLength(0);
  button('Try again').click();
  await flush();
  expect(rows()).toHaveLength(3);
  expect(ipc.count('managed_sessions_page')).toBe(2);
  // A page that is not the projection is a failed read as well, not data.
  await unmount(component!);
  document.body.replaceChildren();
  await start({ page: () => ({ sessions: [{ id: 'x', secret: 'leak' }] }) });
  expect(view().querySelector('[role="alert"]')?.textContent).toContain(
    'Nothing is shown rather than something unverified.',
  );
  expect(view().textContent).not.toContain('leak');
});

it('shows the details of the selected session with the actions its state allows', async () => {
  await start();
  rows()[1].click();
  await flush();
  expect(rows()[1].getAttribute('aria-pressed')).toBe('true');
  const panel = details()!;
  expect(panel.querySelector('h3')?.textContent).toBe('rag-corp');
  const facts = Object.fromEntries(
    Array.from(panel.querySelectorAll('dt')).map((dt) => [
      dt.textContent,
      dt.nextElementSibling?.textContent?.replace(/\s+/g, ' ').trim(),
    ]),
  );
  expect(facts.Agent).toBe('Codex');
  expect(facts.Runtime).toContain('Detached');
  expect(facts.Workspace).toBe('/home/dev/rag-corp');
  expect(facts.Revision).toBe('1');
  expect(facts.Session).toBe(bound.id);
  // Bound, detached and with a ready agent: both actions are offered.
  expect(button('Resume', panel).disabled).toBe(false);
  expect(button('Close', panel).disabled).toBe(false);
  expect(panel.querySelector('.session-hint')).toBeNull();

  // No workspace yet: there is nothing to resume, and activating is what is offered.
  rows()[0].click();
  await flush();
  expect(button('Resume', details()!)).toBeUndefined();
  expect(button('Activate', details()!).disabled).toBe(false);
  expect(text(details()!.querySelector('.session-hint'))).toContain(
    'Activate binds a project folder and attaches the session to Codex.',
  );
  expect(button('Close', details()!).disabled).toBe(false);
  // Closed: nothing to do with it.
  rows()[2].click();
  await flush();
  expect(button('Activate', details()!)).toBeUndefined();
  expect(button('Resume', details()!).disabled).toBe(true);
  expect(button('Close', details()!).disabled).toBe(true);
  expect(details()!.querySelector('.session-hint')?.textContent).toContain(
    'closed session cannot be resumed',
  );
  // Selecting it again hides the panel, and so does the panel's own control.
  rows()[2].click();
  await flush();
  expect(details()).toBeNull();
  rows()[1].click();
  await flush();
  details()!
    .querySelector<HTMLButtonElement>('[aria-label="Hide details"]')!
    .click();
  await flush();
  expect(details()).toBeNull();
  expect(rows().some((r) => r.getAttribute('aria-pressed') === 'true')).toBe(
    false,
  );
});

it('does not offer resume while the agent runtime is not ready', async () => {
  const ipc = await start({
    authority: (after) => (after === null ? views.connected : never()),
  });
  rows()[1].click();
  await flush();
  expect(button('Resume', details()!).disabled).toBe(true);
  expect(details()!.querySelector('.session-hint')?.textContent).toContain(
    'agent runtime is not ready',
  );
  button('Resume', details()!).click();
  await flush();
  expect(ipc.sent()).toHaveLength(0);
});

it('creates and activates a session from one composer, step by step', async () => {
  const session = '44444444-4444-4444-8444-444444444444';
  const row = (binding: string) => ({
    ...fresh,
    id: session,
    position: '10',
    binding,
    workspace: binding === 'unbound' ? null : FOLDER,
    resumable: binding === 'bound_detached',
    activatable: binding === 'unbound',
  });
  const page = (binding: string, head: string) => ({
    ...pages.loaded,
    head,
    sessions: [row(binding), ...pages.loaded.sessions],
  });
  let shown: unknown = pages.loaded;
  const views_: ((view: unknown) => void)[] = [];
  const answers: ((value: unknown) => void)[] = [];
  const pending = () => new Promise((resolve) => answers.push(resolve));
  const stopped = {
    ...ready,
    agent: { ...ready.agent, actual: 'stopped', model_count: 0 },
  };
  const ipc = await start({
    authority: () => new Promise((resolve) => views_.push(resolve)),
    page: () => shown,
    intent: pending,
    activate: pending,
  });
  views_.shift()!(stopped);
  await flush();
  const status = () => text(view().querySelector('.sessions-status'));

  button('New session').click();
  await flush();
  const composer = () => view().querySelector('.session-composer')!;
  // A project and an agent, and nothing to type: the folder comes from the dialog.
  expect(text(composer())).toContain('Project No folder selected');
  expect(text(composer())).toContain('Agent Codex');
  expect(
    composer().querySelectorAll('select, textarea, input:not([type="radio"])'),
  ).toHaveLength(0);
  // The one choice it offers is the mode, and it starts at Inference.
  const modes = Array.from(
    composer().querySelectorAll<HTMLInputElement>('input[type="radio"]'),
  );
  expect(modes.map((m) => [m.value, m.checked])).toEqual([
    ['inference', true],
    ['local_tools', false],
  ]);
  expect(composer().querySelector('.composer-note.caution')).toBeNull();
  expect(button('New session').disabled).toBe(true);
  expect(button('Create & activate', composer()).disabled).toBe(true);
  button('Create & activate', composer()).click();
  await flush();
  expect(ipc.sent()).toHaveLength(0);

  button('Choose folder', composer()).click();
  await flush();
  // The dialog was asked for with nothing, and what it returned is what is shown.
  expect(ipc.calls.filter((c) => c.command === 'choose_workspace')).toEqual([
    { command: 'choose_workspace', request: {} },
  ]);
  expect(text(composer())).toContain(`Project ${FOLDER}`);
  expect(button('Change folder', composer())).toBeDefined();
  button('Create & activate', composer()).click();
  await flush();

  // Pending: said so, and nothing can be started twice.
  expect(status()).toBe('Preparing session…');
  expect(view().querySelector('.session-composer')).toBeNull();
  expect(button('New session').disabled).toBe(true);
  button('New session').click();
  await flush();
  let sent = ipc.sent();
  expect(sent).toHaveLength(1);
  expect(sent[0].command).toBe('managed_session_prepare');
  expect(Object.keys(sent[0].request)).toEqual(['intent_id']);
  const action = sent[0].request.intent_id;
  expect(action).toMatch(UUID);

  // The session exists: it is read and selected, and Codex is not running yet.
  shown = page('unbound', '10');
  answers.shift()!({ id: session });
  await flush();
  expect(rows()).toHaveLength(4);
  expect(rows()[0].getAttribute('aria-pressed')).toBe('true');
  expect(status()).toBe('Starting Codex…');
  sent = ipc.sent();
  expect(sent).toHaveLength(2);
  expect(sent[1]).toEqual({
    command: 'managed_session_activate',
    // One action: the identity the session was prepared under, the folder as chosen,
    // and the mode: Inference, because nothing else was asked for.
    request: {
      operation_id: action,
      session_id: session,
      expected_revision: 1,
      workspace: FOLDER,
      mode: 'inference',
    },
  });
  answers.shift()!({ stage: 'runtime_starting' });
  await flush();
  // Waiting on the authority: nothing is sent, and nothing is shown as further along.
  expect(status()).toBe('Starting Codex…');
  expect(ipc.sent()).toHaveLength(2);
  expect(rows()[0].textContent).toContain('No workspace');

  // The authority reports Codex ready: the same request goes again, for the binding.
  views_.shift()!({ ...ready, revision: 5 });
  await flush();
  expect(status()).toBe('Creating workspace binding…');
  expect(ipc.sent()).toHaveLength(3);
  expect(ipc.sent()[2]).toEqual(sent[1]);
  shown = page('bound_detached', '12');
  answers.shift()!({ stage: 'bound' });
  await flush();
  expect(status()).toBe('Attaching…');
  expect(rows()[0].textContent).toContain('Detached');
  expect(ipc.sent()).toHaveLength(4);
  expect(ipc.sent()[3]).toEqual(sent[1]);

  shown = page('bound_attached', '14');
  answers.shift()!({ stage: 'ready' });
  await flush();
  // Ready, because the page that was read says so.
  expect(view().querySelector('.sessions-status')).toBeNull();
  expect(rows()[0].textContent).toContain('Ready');
  expect(rows()[0].textContent).toContain(FOLDER);
  const facts = text(details()!.querySelector('.session-facts'));
  expect(facts).toContain('Runtime Ready');
  expect(text(details()!.querySelector('.session-hint'))).toContain(
    'Ready: attached to the running agent.',
  );
  expect(button('New session').disabled).toBe(false);
  // One session, one folder dialog, four steps of one action.
  expect(ipc.sent().map((c) => c.command)).toEqual([
    'managed_session_prepare',
    'managed_session_activate',
    'managed_session_activate',
    'managed_session_activate',
  ]);
});

it('activates an existing session that has no workspace', async () => {
  const ipc = await start({
    activate: (_, step) => ({ stage: step === 1 ? 'bound' : 'ready' }),
  });
  rows()[0].click();
  await flush();
  button('Activate', details()!).click();
  await flush();
  const composer = view().querySelector('.session-composer')!;
  expect(composer.getAttribute('aria-label')).toBe('Activate session');
  expect(text(composer)).toContain('Activate Session 33333333');
  expect(button('Create & activate', composer)).toBeUndefined();
  expect(button('Activate', composer).disabled).toBe(true);
  // While it is open, the same composer is not opened twice.
  expect(button('Activate', details()!).disabled).toBe(true);
  button('Choose folder', composer).click();
  await flush();
  button('Activate', composer).click();
  await flush();
  const sent = ipc.sent();
  // No session was created: the one that was shown, at the revision it was shown at.
  expect(sent.map((c) => c.command)).toEqual([
    'managed_session_activate',
    'managed_session_activate',
  ]);
  expect(sent[0].request).toEqual({
    operation_id: expect.stringMatching(UUID),
    session_id: fresh.id,
    expected_revision: 1,
    workspace: FOLDER,
    mode: 'inference',
  });
  expect(sent[1].request).toEqual(sent[0].request);
  expect(view().querySelector('.sessions-status')).toBeNull();
});

it('keeps a dismissed dialog and a folder that cannot be used out of the request', async () => {
  const outcomes: (() => unknown)[] = [
    () => null,
    () => Promise.reject({ code: 'workspace_invalid', retry: false }),
    () => Promise.reject({ code: 'internal', retry: false }),
  ];
  const ipc = await start({ choose: () => outcomes.shift()!() });
  button('New session').click();
  await flush();
  const composer = () => view().querySelector('.session-composer')!;
  button('Choose folder', composer()).click();
  await flush();
  // Dismissed: nothing chosen, nothing said, nothing to send.
  expect(text(composer())).toContain('No folder selected');
  expect(composer().querySelector('[role="alert"]')).toBeNull();
  for (const sentence of [
    'That folder cannot be used',
    'The folder dialog could not be opened.',
  ]) {
    button('Choose folder', composer()).click();
    await flush();
    expect(text(composer().querySelector('[role="alert"]'))).toContain(
      sentence,
    );
    expect(text(composer())).toContain('No folder selected');
    expect(button('Create & activate', composer()).disabled).toBe(true);
  }
  button('Cancel', composer()).click();
  await flush();
  expect(view().querySelector('.session-composer')).toBeNull();
  expect(ipc.sent()).toHaveLength(0);
});

it('explains a failed action and retries it as the same action', async () => {
  const outcomes = [
    () => Promise.reject({ code: 'unavailable', retry: true }),
    () => ({ id: fresh.id }),
  ];
  const steps = [
    () => Promise.reject({ code: 'workspace_invalid', retry: false }),
    () => Promise.reject({ code: 'uncertain', retry: false }),
  ];
  const ipc = await start({
    page: () => pages.empty,
    intent: () => outcomes.shift()!(),
    activate: () => steps.shift()!(),
  });
  const activate = async () => {
    button('New session').click();
    await flush();
    button('Choose folder').click();
    await flush();
    button('Create & activate').click();
    await flush();
  };
  await activate();
  let alert = view().querySelector('.sessions-status.failed')!;
  expect(alert.getAttribute('role')).toBe('alert');
  expect(alert.textContent).toContain('The authority did not answer');
  // An answer that never came is a caution, not an error.
  expect(alert.classList.contains('unknown')).toBe(true);
  // Product wording only.
  expect(alert.textContent).not.toMatch(/unavailable|_|\{/);
  button('Retry', alert).click();
  await flush();
  let sent = ipc.sent();
  // The same action: the same intent, so the authority can never do it twice.
  expect(sent[1].request).toEqual(sent[0].request);
  expect(sent[2].command).toBe('managed_session_activate');
  expect(sent[2].request.operation_id).toBe(sent[0].request.intent_id);

  // A refusal has no retry: it is read, dismissed, and a new click is a new action.
  alert = view().querySelector('.sessions-status.failed')!;
  expect(alert.textContent).toContain('The session is not ready.');
  expect(alert.textContent).toContain('did not accept that folder');
  expect(alert.classList.contains('unknown')).toBe(false);
  expect(button('Retry', alert)).toBeUndefined();
  button('Dismiss', alert).click();
  await flush();
  expect(view().querySelector('.sessions-status')).toBeNull();

  // An outcome the authority could not confirm is said as that, and is not retried.
  outcomes.push(() => ({ id: fresh.id }));
  await activate();
  sent = ipc.sent();
  expect(sent.at(-2)!.request.intent_id).not.toBe(sent[0].request.intent_id);
  alert = view().querySelector('.sessions-status.failed')!;
  expect(alert.textContent).toContain('Activation outcome uncertain');
  expect(alert.classList.contains('unknown')).toBe(true);
  expect(alert.textContent).not.toMatch(/not ready|refused|failed|_/);
  expect(button('Retry', alert)).toBeUndefined();
  expect(ipc.sent()).toHaveLength(sent.length);
});

it('shows an uncertain activation as uncertain and discards it only when asked', async () => {
  const uncertain = {
    ...pages.loaded,
    sessions: [
      {
        ...fresh,
        binding: 'indeterminate',
        activatable: false,
        recoverable: true,
      },
      bound,
      closed,
    ],
  };
  const ipc = await start({
    page: (_, read) => (read === 1 ? uncertain : pages.loaded),
  });
  expect(rows()[0].textContent).toContain('Uncertain');
  rows()[0].click();
  await flush();
  const panel = () => details()!;
  expect(text(panel().querySelector('.session-uncertain'))).toContain(
    'Activation outcome uncertain',
  );
  // Neither activating nor resuming is offered: nothing asks Codex again.
  expect(button('Activate', panel())).toBeUndefined();
  expect(button('Resume', panel())).toBeUndefined();
  button('Discard attempt', panel()).click();
  await flush();
  // Asked about first, and said plainly: what Codex did stays unknown.
  expect(ipc.sent()).toHaveLength(0);
  const confirm = panel().querySelector('.close-confirm')!;
  expect(confirm.getAttribute('aria-label')).toBe('Confirm discard');
  expect(text(confirm)).toContain('the empty thread is left behind');
  button('Keep it', confirm).click();
  await flush();
  expect(panel().querySelector('.close-confirm')).toBeNull();
  expect(ipc.sent()).toHaveLength(0);

  button('Discard attempt', panel()).click();
  await flush();
  button('Discard attempt', panel().querySelector('.close-confirm')!).click();
  await flush();
  expect(ipc.sent()).toEqual([
    {
      command: 'managed_session_abandon',
      // The session and the action. The attempt is the authority's to name.
      request: {
        intent_id: expect.stringMatching(UUID),
        session_id: fresh.id,
      },
    },
  ]);
  // Read again: the session has no workspace and can be activated.
  expect(rows()[0].textContent).toContain('No workspace');
  expect(button('Activate', panel()).disabled).toBe(false);
});

it('goes on activating while the Console is shown, without a second request', async () => {
  const answers: ((value: unknown) => void)[] = [];
  const ipc = await start({
    activate: () => new Promise((resolve) => answers.push(resolve)),
  });
  rows()[0].click();
  await flush();
  button('Activate', details()!).click();
  await flush();
  button('Choose folder').click();
  await flush();
  const composer = view().querySelector('.session-composer')!;
  const go = button('Activate', composer);
  // A double click is one action.
  go.click();
  go.click();
  await flush();
  expect(ipc.sent()).toHaveLength(1);
  for (let visit = 0; visit < 4; visit++) {
    await open('Console');
    await open('Sessions');
  }
  expect(ipc.sent()).toHaveLength(1);
  expect(text(view().querySelector('.sessions-status'))).toBe(
    'Creating workspace binding…',
  );
  await open('Console');
  answers.shift()!({ stage: 'bound' });
  await flush();
  // Still the same action, one step further, with the terminal untouched.
  expect(ipc.sent()).toHaveLength(2);
  expect(ipc.sent()[1].request).toEqual(ipc.sent()[0].request);
  answers.shift()!({ stage: 'ready' });
  await flush();
  await open('Sessions');
  expect(view().querySelector('.sessions-status')).toBeNull();
  expect(ipc.count('create_terminal')).toBe(1);
  expect(ipc.count('close_terminal')).toBe(0);
  expect(ipc.count('authority_state')).toBe(2);
});

it('asks before closing and sends the session and revision that were shown', async () => {
  const ipc = await start({ intent: () => ({ id: bound.id }) });
  rows()[1].click();
  await flush();
  button('Close', details()!).click();
  await flush();
  // Nothing was sent: closing is asked about first.
  expect(ipc.sent()).toHaveLength(0);
  const confirm = details()!.querySelector('.close-confirm')!;
  expect(confirm.textContent).toContain('It cannot be reopened');
  expect(button('Resume', details()!)).toBeUndefined();
  button('Keep open', confirm).click();
  await flush();
  expect(details()!.querySelector('.close-confirm')).toBeNull();
  expect(ipc.sent()).toHaveLength(0);

  button('Close', details()!).click();
  await flush();
  button('Close session', details()!).click();
  await flush();
  const sent = ipc.sent();
  expect(sent).toHaveLength(1);
  expect(sent[0].command).toBe('managed_session_close');
  expect(sent[0].request).toEqual({
    intent_id: expect.stringMatching(UUID),
    session_id: bound.id,
    expected_revision: 1,
  });
  // Accepted and read again; the fixture still says prepared, and so does the view:
  // what is shown is what was read, never what was hoped.
  expect(ipc.count('managed_sessions_page')).toBe(2);
  expect(rows()[1].querySelector('.pill')?.textContent?.trim()).toBe(
    'Prepared',
  );
});

it('resumes a bound session with one narrow request', async () => {
  const attached = {
    ...pages.loaded,
    head: '11',
    sessions: [
      fresh,
      { ...bound, binding: 'bound_attached', resumable: false },
      closed,
    ],
  };
  const ipc = await start({
    page: (_, read) => (read === 1 ? pages.loaded : attached),
    intent: () => ({ id: bound.id }),
  });
  rows()[1].click();
  await flush();
  button('Resume', details()!).click();
  await flush();
  const sent = ipc.sent();
  expect(sent).toHaveLength(1);
  expect(sent[0].command).toBe('managed_session_resume');
  // No scope, lease, lifetime or workspace is the window's to send.
  expect(sent[0].request).toEqual({
    intent_id: expect.stringMatching(UUID),
    session_id: bound.id,
    expected_revision: 1,
  });
  expect(rows()[1].textContent).toContain('Ready');
  expect(button('Resume', details()!).disabled).toBe(true);
  expect(details()!.querySelector('.session-hint')?.textContent).toContain(
    'Ready: attached to the running agent.',
  );
});

it('keeps the page reachable and every mutation disabled while the authority is offline', async () => {
  const answers: ((view: unknown) => void)[] = [];
  const ipc = await start({
    authority: () => new Promise((resolve) => answers.push(resolve)),
  });
  // Not answered yet: connecting, not an endless spinner over stale data.
  expect(view().textContent).toContain('Connecting to the authority…');
  answers.shift()!(views.waiting_absent);
  await flush();
  const offline = view().querySelector('.sessions-empty')!;
  expect(offline.textContent).toContain('The authority is unavailable');
  expect(offline.textContent).toContain('The authority is not running.');
  expect(offline.textContent).toContain('The Console keeps working.');
  expect(button('New session').disabled).toBe(true);
  expect(button('Refresh').disabled).toBe(true);
  expect(ipc.count('managed_sessions_page')).toBe(0);
  expect(
    document.querySelectorAll('.terminal-feature [role="tab"]'),
  ).toHaveLength(1);

  // The authority arrives: its sessions are read. It leaves: they are gone at once.
  answers.shift()!(ready);
  await flush();
  expect(rows()).toHaveLength(3);
  rows()[1].click();
  await flush();
  button('Close', details()!).click();
  await flush();
  answers.shift()!({ ...views.waiting_absent, revision: 7, reason: 'lost' });
  await flush();
  expect(rows()).toHaveLength(0);
  expect(details()).toBeNull();
  expect(view().textContent).toContain('The authority stopped answering.');
  expect(button('New session').disabled).toBe(true);
  expect(ipc.sent()).toHaveLength(0);
  // Back, as another instance: read again, with the selection kept.
  answers.shift()!({
    ...ready,
    revision: 9,
    authority: { ...ready.authority, restarts: 1 },
  });
  await flush();
  expect(rows()).toHaveLength(3);
  expect(rows()[1].getAttribute('aria-pressed')).toBe('true');
  expect(details()!.querySelector('.close-confirm')).toBeNull();
  expect(ipc.count('managed_sessions_page')).toBe(2);
});

it('refreshes from the one authority follower when a durable event arrives', async () => {
  const answers: ((view: unknown) => void)[] = [];
  const ipc = await start({
    authority: () => new Promise((resolve) => answers.push(resolve)),
    page: (_, read) =>
      read === 1 ? pages.loaded : { ...pages.loaded, head: '10', sessions: [] },
  });
  answers.shift()!(ready);
  await flush();
  expect(rows()).toHaveLength(3);
  expect(ipc.count('managed_sessions_page')).toBe(1);
  // A view change that is not a new event reads nothing.
  answers.shift()!({
    ...ready,
    revision: 4,
    agent: { ...ready.agent, actual: 'stopping', model_count: 0 },
  });
  await flush();
  expect(ipc.count('managed_sessions_page')).toBe(1);
  // A new head does, once.
  answers.shift()!({
    ...ready,
    revision: 5,
    authority: { ...ready.authority, last_sequence: '10' },
  });
  await flush();
  expect(ipc.count('managed_sessions_page')).toBe(2);
  expect(rows()).toHaveLength(0);
  // One follower all along: each answer was followed by exactly one next read.
  expect(ipc.count('authority_state')).toBe(4);
});

it('pages through older sessions without keeping the pages it left', async () => {
  const newest = { ...pages.loaded, older: '1' };
  const ipc = await start({
    page: (before) =>
      before === null ? newest : { ...pages.loaded, sessions: [] },
  });
  const pager = () => view().querySelector('.sessions-pager')!;
  expect(pager().textContent).toContain('Newest sessions');
  expect(button('Newer', pager()).disabled).toBe(true);
  button('Older', pager()).click();
  await flush();
  expect(ipc.sent('managed_sessions_page').at(-1)!.request).toEqual({
    before: '1',
  });
  expect(rows()).toHaveLength(0);
  expect(pager().textContent).toContain('Older sessions · page 2');
  expect(button('Older', pager()).disabled).toBe(true);
  button('Newer', pager()).click();
  await flush();
  expect(rows()).toHaveLength(3);
  expect(ipc.sent('managed_sessions_page').at(-1)!.request).toEqual({
    before: null,
  });
});

it('keeps the terminal workspace alive across every visit to Sessions', async () => {
  const ipc = await start();
  const terminal = () =>
    document.querySelector<HTMLElement>('.terminal-feature')!;
  for (let visit = 0; visit < 12; visit++) {
    // In Sessions: the terminal workspace is out of sight and still mounted.
    expect(terminal().hidden).toBe(true);
    expect(terminal().inert).toBe(true);
    expect(view().hidden).toBe(false);
    expect(
      document.querySelectorAll('.terminal-feature [role="tab"]'),
    ).toHaveLength(1);
    await open('Console');
    expect(terminal().hidden).toBe(false);
    expect(terminal().inert).toBe(false);
    expect(view().hidden).toBe(true);
    expect(document.querySelector('.pill.terminal')?.textContent).toContain(
      'Running',
    );
    await open('Sessions');
  }
  // One terminal, created once and never closed; one renderer; one authority follower
  // parked on one call; and one page read, because nothing changed in between.
  expect(ipc.count('create_terminal')).toBe(1);
  expect(ipc.count('close_terminal')).toBe(0);
  expect(fakes.factory).toHaveBeenCalledTimes(1);
  expect(ipc.count('authority_state')).toBe(2);
  expect(ipc.count('managed_sessions_page')).toBe(1);
  expect(rows()).toHaveLength(3);
  // The same DOM node all along: the workspace was hidden, never rebuilt.
  expect(document.querySelectorAll('.terminal-feature')).toHaveLength(1);
  expect(document.querySelectorAll('.sessions-feature')).toHaveLength(1);
});

/** The fixture page with `bound` attached in `mode` and the runtime serving it. */
const attachedIn = (mode: 'inference' | 'local_tools') => ({
  ...pages.loaded,
  runtime:
    mode === 'local_tools'
      ? { mode, workspace: bound.workspace }
      : { mode, workspace: null },
  sessions: [fresh, { ...bound, binding: 'bound_attached', mode }, closed],
});

it('creates a session in Local Tools only when that mode is chosen, and says what it means', async () => {
  const stopped = {
    ...ready,
    agent: { ...ready.agent, actual: 'stopped', model_count: 0 },
  };
  const ipc = await start({
    authority: (after) => (after === null ? stopped : never()),
    activate: () => ({ stage: 'runtime_starting' }),
  });
  button('New session').click();
  await flush();
  const composer = () =>
    view().querySelector<HTMLElement>('.session-composer')!;
  const modes = () =>
    Array.from(
      composer().querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    );
  // Both modes are named with what they are, and Inference is the one selected.
  expect(text(composer().querySelector('.mode-choice'))).toBe(
    'Mode Inference Text only. Codex is offered no tool. Local Tools Codex may propose commands and file changes in this folder. Each one waits for your approval.',
  );
  expect(modes().map((m) => m.checked)).toEqual([true, false]);
  modes()[1].click();
  await flush();
  expect(modes().map((m) => m.checked)).toEqual([false, true]);
  // More capability is not more authority, and the runtime is one: both are said
  // before anything is asked.
  expect(text(composer())).toMatch(
    /not more authority: every command and file change waits for your approval\. The authority runs one Codex runtime, in one mode and for one folder at a time\./,
  );
  // No runtime is running: nothing is restarted, and nothing says it would be.
  expect(composer().querySelector('.composer-note.caution')).toBeNull();

  button('Choose folder', composer()).click();
  await flush();
  button('Create & activate', composer()).click();
  await flush();
  const sent = ipc.sent('managed_session_activate');
  // The mode is a name next to the folder: no profile, no path of its own.
  expect(sent[0].request).toEqual({
    operation_id: expect.stringMatching(UUID),
    session_id: fresh.id,
    expected_revision: 1,
    workspace: FOLDER,
    mode: 'local_tools',
  });
  expect(text(view().querySelector('.sessions-status'))).toBe(
    'Starting Codex in Local Tools…',
  );
});

it('says what activating does to the runtime that is running, before it is asked', async () => {
  const composer = () =>
    view().querySelector<HTMLElement>('.session-composer')!;
  const caution = () =>
    text(composer().querySelector('.composer-note.caution'));
  const modes = () =>
    Array.from(
      composer().querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    );
  // Codex runs in Local Tools for another folder, with a session attached to it.
  const ipc = await start({
    page: () => attachedIn('local_tools'),
    activate: () => ({ stage: 'runtime_stopping' }),
  });
  button('New session').click();
  await flush();
  // Inference is what is selected, and here it is the mode that costs a restart.
  expect(modes().map((m) => m.checked)).toEqual([true, false]);
  expect(caution()).toBe(
    'Codex runs in Local Tools for rag-corp now, and the authority runs one Codex runtime. Activating this session restarts it in Inference, and the sessions attached to it are detached. A session created in Local Tools cannot be resumed in Inference.',
  );
  // The action is named for what it does. Nothing is asked until it is pressed.
  expect(button('Create & activate', composer())).toBeUndefined();
  expect(button('Restart Codex & activate', composer())!.disabled).toBe(true);
  expect(ipc.sent('managed_session_activate')).toHaveLength(0);
  // Local Tools for another folder is a restart as well: one runtime, one folder.
  modes()[1].click();
  await flush();
  button('Choose folder', composer()).click();
  await flush();
  expect(caution()).toBe(
    'Codex runs in Local Tools for rag-corp now, and the authority runs one Codex runtime. Activating this session restarts it in Local Tools for this folder, and the sessions attached to it are detached.',
  );
  button('Restart Codex & activate', composer())!.click();
  await flush();
  expect(ipc.sent('managed_session_activate')[0].request).toMatchObject({
    workspace: FOLDER,
    mode: 'local_tools',
  });
  expect(text(view().querySelector('.sessions-status'))).toBe(
    'Stopping the managed runtime…',
  );
});

it('announces no restart when the runtime already runs what is asked', async () => {
  const composer = () =>
    view().querySelector<HTMLElement>('.session-composer')!;
  await start({ page: () => attachedIn('inference') });
  button('New session').click();
  await flush();
  expect(composer().querySelector('.composer-note.caution')).toBeNull();
  expect(button('Create & activate', composer())).toBeDefined();
  // Local Tools from a runtime in Inference: said as soon as it is chosen.
  composer()
    .querySelectorAll<HTMLInputElement>('input[type="radio"]')[1]
    .click();
  await flush();
  expect(text(composer().querySelector('.composer-note.caution'))).toBe(
    'Codex runs in Inference now, and the authority runs one Codex runtime. Activating this session restarts it in Local Tools for this folder, and the sessions attached to it are detached. A session created in Inference cannot be resumed in Local Tools.',
  );
});

it('shows the mode a session is attached in and the runtime every session shares', async () => {
  await start({ page: () => attachedIn('local_tools') });
  expect(text(view().querySelector('.sessions-count'))).toBe(
    '1 prepared · 0 closed · Codex runs in Local Tools for rag-corp',
  );
  expect(text(rows()[1])).toContain('Codex · Ready · Local Tools');
  // Not attached: no mode is claimed for it.
  expect(text(rows()[0])).not.toMatch(/Local Tools|Inference/);
  rows()[1].click();
  await flush();
  const facts = () => text(details()!.querySelector('.session-facts'));
  expect(facts()).toContain('Mode Local Tools');
  expect(text(details())).toContain(
    'This session runs in Local Tools. Every command and file change Codex proposes waits for your approval.',
  );
  // Attached: there is no mode to change to, and nothing offers one.
  expect(button('Enable Local Tools', details()!)).toBeUndefined();
  expect(button('Use Inference', details()!)).toBeUndefined();
  rows()[0].click();
  await flush();
  expect(facts()).toContain('Mode Not attached');
});

it('explains that an Inference session stays one, and opens a Local Tools composer by name', async () => {
  await start({ page: () => attachedIn('inference') });
  expect(text(view().querySelector('.sessions-count'))).toContain(
    'Codex runs in Inference',
  );
  rows()[1].click();
  await flush();
  expect(text(details()!.querySelector('.session-facts'))).toContain(
    'Mode Inference',
  );
  // Its conversation cannot move: nothing offers to move it.
  expect(button('Enable Local Tools', details()!)).toBeUndefined();
  expect(text(details())).toContain(
    'Local Tools needs a session of its own, because a conversation stays in the mode it was created in.',
  );
  button('New Local Tools session', details()!)!.click();
  await flush();
  const composer = view().querySelector<HTMLElement>('.session-composer')!;
  expect(text(composer.querySelector('.composer-title'))).toBe(
    'New managed session',
  );
  expect(
    Array.from(
      composer.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    ).map((m) => m.checked),
  ).toEqual([false, true]);
  // No folder was carried over: one is chosen in the dialog, as always.
  expect(text(composer)).toContain('Project No folder selected');
});

it('changes the mode of a bound session only after saying what it does', async () => {
  const answers: ((value: unknown) => void)[] = [];
  const ipc = await start({
    page: () => ({
      ...pages.loaded,
      runtime: { mode: 'inference', workspace: null },
    }),
    mode: () => new Promise((resolve) => answers.push(resolve)),
  });
  rows()[1].click();
  await flush();
  // Bound and detached, with Inference running: Local Tools can be asked for.
  expect(button('Use Inference', details()!)).toBeUndefined();
  button('Enable Local Tools', details()!)!.click();
  await flush();
  const confirm = () =>
    details()!.querySelector<HTMLElement>('[aria-label="Confirm mode"]')!;
  expect(text(confirm())).toBe(
    'Enable Local Tools for rag-corp? This restarts the managed Codex runtime in Local Tools for this workspace. Sessions attached now are detached. A session created in Inference cannot be resumed in Local Tools. Every command and file change still waits for your approval. Enable Local Tools Cancel',
  );
  // Nothing was asked yet, and Cancel asks nothing.
  expect(ipc.count('managed_session_mode')).toBe(0);
  button('Cancel', confirm())!.click();
  await flush();
  expect(details()!.querySelector('[aria-label="Confirm mode"]')).toBeNull();
  expect(ipc.count('managed_session_mode')).toBe(0);

  button('Enable Local Tools', details()!)!.click();
  await flush();
  button('Enable Local Tools', confirm())!.click();
  await flush();
  const sent = ipc.sent('managed_session_mode');
  // The session, the revision that was shown and the mode. No folder: the workspace
  // is the one the authority holds for the session.
  expect(sent).toHaveLength(1);
  expect(sent[0].request).toEqual({
    operation_id: expect.stringMatching(UUID),
    session_id: bound.id,
    expected_revision: 1,
    mode: 'local_tools',
  });
  // The runtime really is being stopped, and that is what is shown.
  answers.shift()!({ stage: 'runtime_stopping' });
  await flush();
  expect(text(view().querySelector('.sessions-status'))).toBe(
    'Stopping the managed runtime…',
  );
  // While it is pending no other session action is offered.
  expect(button('Resume', details()!)!.disabled).toBe(true);
  expect(button('Enable Local Tools', details()!)!.disabled).toBe(true);
});

it('says why a mode could not be entered, in the words of what happened', async () => {
  const failures: Record<string, RegExp> = {
    turn_active:
      /A turn is running, and the runtime it runs on is not restarted under it/,
    rejected:
      /Codex refused to resume this session in that mode\. A session resumes only in the mode it was created in/,
    start_refused:
      /refused to start Codex in Local Tools: it could not set up or prove the isolation that mode requires\. Nothing runs without it/,
    mode_fixed: /attached in the other mode/,
  };
  for (const [code, message] of Object.entries(failures)) {
    await start({
      page: () => ({
        ...pages.loaded,
        runtime: { mode: 'inference', workspace: null },
      }),
      mode: () => Promise.reject({ code, retry: false }),
    });
    rows()[1].click();
    await flush();
    button('Enable Local Tools', details()!)!.click();
    await flush();
    button(
      'Enable Local Tools',
      details()!.querySelector<HTMLElement>('[aria-label="Confirm mode"]')!,
    )!.click();
    await flush();
    const alert = view().querySelector('.sessions-status.failed')!;
    expect(text(alert)).toMatch(/^The session is not ready in that mode\./);
    expect(text(alert)).toMatch(message);
    // A refusal offers no Retry: sending it again would change nothing.
    expect(button('Retry', alert)).toBeUndefined();
    await unmount(component!);
    component = undefined;
    await flush();
  }
});
