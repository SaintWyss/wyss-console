import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import type { TurnSummary } from '../src/lib/contracts/turns';
import { listing, reading, S, S2, T1, T2, turn } from './agent-harness';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v3.json';

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
const [fresh, bound] = pages.loaded.sessions;
const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const flush = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
  flushSync();
};
const never = () => new Promise(() => undefined);
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const ready = (head: string, revision = 3) => ({
  ...views.connected,
  revision,
  authority: { ...views.connected.authority, last_sequence: head },
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
});
/** Two Ready sessions and one that has no workspace yet. */
const page = {
  ...pages.loaded,
  sessions: [
    fresh,
    { ...bound, binding: 'bound_attached' },
    {
      ...bound,
      id: S2,
      binding: 'bound_attached',
      position: '3',
      workspace: '/home/dev/other',
    },
  ],
};

interface Call {
  command: string;
  request: Record<string, unknown>;
  resolve: (value: unknown) => void;
  reject: (failure: unknown) => void;
}
/**
 * A desktop whose terminal works, whose sessions are `page`, and whose turns are what
 * the test says: lists are answered from `turns`, and every other turn call waits for
 * the test to answer it.
 */
function desktop() {
  const calls: Call[] = [];
  const world = {
    /** Newest first, by session. */
    turns: new Map<string, TurnSummary[]>(),
    /** The authority read that is waiting for a change. */
    authority: undefined as ((view: unknown) => void) | undefined,
  };
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    let resolve!: (value: unknown) => void;
    let reject!: (failure: unknown) => void;
    const answer = new Promise((res, rej) => {
      resolve = res;
      reject = rej;
    });
    calls.push({ command, request, resolve, reject });
    if (command === 'authority_state') {
      if (request.after === null) return ready('9');
      world.authority = resolve;
      return answer;
    }
    if (command === 'managed_sessions_page') return page;
    if (command === 'managed_turns') {
      const session = request.session_id as typeof S;
      return listing(world.turns.get(session) ?? [], {
        session: session === S2 ? { id: S2, workspace: '/home/dev/other' } : {},
      });
    }
    if (command.startsWith('managed_turn_')) return answer;
    if (command === 'terminal_read_output') return never();
    if (command === 'get_app_info')
      return { schema_version: 1, name: 'Wyss Console', version: '2.0-test' };
    return info;
  });
  const of = (command: string) => calls.filter((c) => c.command === command);
  return {
    calls,
    world,
    of,
    count: (command: string) => of(command).length,
    last: (command: string) => of(command).at(-1)!,
  };
}
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label))!;
const agent = () => document.querySelector<HTMLElement>('.agent-feature')!;
const sessions = () =>
  document.querySelector<HTMLElement>('.sessions-feature')!;
const button = (label: string, root: ParentNode = agent()) =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  );
const box = () => agent().querySelector<HTMLTextAreaElement>('textarea')!;
const turns = () => Array.from(agent().querySelectorAll('.turn'));
async function type(value: string) {
  box().value = value;
  box().dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
}
async function press(init: KeyboardEventInit) {
  const event = new KeyboardEvent('keydown', {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  box().dispatchEvent(event);
  await flush();
  return event;
}
async function click(target: HTMLElement | undefined) {
  expect(target).toBeDefined();
  target!.click();
  await flush();
}
/** The application with the session at `row` of Sessions opened in the Agent view. */
async function opened(row = 1) {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(row);
  return ipc;
}
async function openAgent(row: number) {
  await click(nav('Sessions'));
  const rows = sessions().querySelectorAll<HTMLButtonElement>('.session-row');
  if (rows[row].getAttribute('aria-pressed') !== 'true') await click(rows[row]);
  await click(button('Open agent', sessions()));
}
/** A turn started from the composer and accepted as `id`. */
async function run(ipc: ReturnType<typeof desktop>, prompt: string, id = T1) {
  await type(prompt);
  await press({ key: 'Enter' });
  ipc.world.turns.set(S, [
    turn(id, 'starting'),
    ...(ipc.world.turns.get(S) ?? []),
  ]);
  ipc.last('managed_turn_start').resolve({ id });
  await flush();
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

it('opens the Agent view from the sidebar and from a session that has a workspace', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  expect(nav('Agents').disabled).toBe(false);
  expect(agent().hidden).toBe(true);
  expect(agent().inert).toBe(true);
  await click(nav('Agents'));
  expect(agent().hidden).toBe(false);
  expect(document.querySelector('.context-title')?.textContent).toBe('Agents');
  // No session is open: there is nothing to write to, and nothing is read.
  expect(text(agent())).toContain('No session is open.');
  expect(agent().querySelector('textarea')).toBeNull();
  expect(ipc.count('managed_turns')).toBe(0);
  await click(button('Go to Sessions'));
  expect(sessions().hidden).toBe(false);

  // A session with no workspace has no agent to open.
  const rows = sessions().querySelectorAll<HTMLButtonElement>('.session-row');
  await click(rows[0]);
  expect(button('Open agent', sessions())).toBeUndefined();
  await click(rows[1]);
  await click(button('Open agent', sessions()));
  expect(agent().hidden).toBe(false);
  expect(sessions().hidden).toBe(true);
  expect(nav('Agents').getAttribute('aria-current')).toBe('page');
  // The session every turn here belongs to is named, with what the authority says of it.
  const header = agent().querySelector('.agent-session');
  expect(text(header)).toBe(
    'rag-corp Ready /home/dev/rag-corp Codex · Inference',
  );
  expect(
    text(agent().querySelector('[role="tab"][aria-selected="true"]')),
  ).toBe('rag-corp');
  expect(ipc.of('managed_turns').map((c) => c.request)).toEqual([
    { session_id: S },
  ]);
  expect(text(agent())).toContain('No turns yet.');
  expect(ipc.count('managed_turn_read')).toBe(0);
  expect(ipc.count('managed_turn_start')).toBe(0);
});

it('runs what the composer holds with Enter, once, and keeps Shift+Enter for a new line', async () => {
  const ipc = await opened();
  expect(box().getAttribute('aria-label')).toBe('Objective');
  expect(button('Run')!.disabled).toBe(true);
  // Nothing to run: Enter sends nothing.
  await press({ key: 'Enter' });
  await type('   ');
  await press({ key: 'Enter' });
  expect(button('Run')!.disabled).toBe(true);
  expect(ipc.count('managed_turn_start')).toBe(0);

  await type('First line');
  expect(button('Run')!.disabled).toBe(false);
  // Shift+Enter is the textarea's own new line: it is not taken and nothing is sent.
  const newline = await press({ key: 'Enter', shiftKey: true });
  expect(newline.defaultPrevented).toBe(false);
  // Text being composed by an input method is not submitted by its Enter.
  await press({ key: 'Enter', isComposing: true });
  expect(ipc.count('managed_turn_start')).toBe(0);
  await type('First line\nSecond line');

  const enter = await press({ key: 'Enter' });
  expect(enter.defaultPrevented).toBe(true);
  const sent = ipc.of('managed_turn_start');
  expect(sent).toHaveLength(1);
  expect(sent[0].request).toEqual({
    intent_id: expect.stringMatching(UUID),
    session_id: S,
    expected_revision: 1,
    prompt: 'First line\nSecond line',
  });
  // Pending: the input is shown as sent, the composer is empty, and Run is not offered.
  expect(box().value).toBe('');
  expect(text(turns().at(-1))).toContain('First line Second line');
  expect(text(turns().at(-1))).toContain(
    'Asking the authority to start the turn…',
  );
  expect(button('Run')!.disabled).toBe(true);
  expect(text(agent().querySelector('.composer-hint'))).toBe(
    'Starting the turn…',
  );
  // Enter, Run and the form again: it is still one start.
  await type('Something else');
  await press({ key: 'Enter' });
  button('Run')!.click();
  agent()
    .querySelector('form')!
    .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await flush();
  expect(ipc.count('managed_turn_start')).toBe(1);
  // The line break of the input is kept as written.
  expect(
    turns().at(-1)!.querySelector('.turn-prompt .turn-text')!.textContent,
  ).toBe('First line\nSecond line');
});

it('shows a turn from its receipt, through its answer as it arrives, to its canonical end', async () => {
  const ipc = await opened();
  await run(ipc, 'Say hello.');
  // Accepted, and not yet anything else.
  expect(turns()).toHaveLength(1);
  expect(text(turns()[0].querySelector('.turn-prompt'))).toBe('You Say hello.');
  expect(ipc.last('managed_turn_read').request).toEqual({
    turn_id: T1,
    after: null,
    wait: false,
  });
  ipc.last('managed_turn_read').resolve(
    reading(turn(T1, 'running', 2), [
      [1, 'started'],
      [2, 'thinking'],
    ]),
  );
  await flush();
  expect(text(turns()[0].querySelector('.turn-head .pill'))).toBe('Running');
  expect(text(turns()[0])).toContain('Thinking…');
  expect(button('Cancel turn')).toBeDefined();
  expect(button('Run')!.disabled).toBe(true);
  expect(text(agent().querySelector('.composer-hint'))).toMatch(
    /A turn is running/,
  );
  expect(ipc.last('managed_turn_read').request).toEqual({
    turn_id: T1,
    after: '2',
    wait: true,
  });

  ipc.last('managed_turn_read').resolve(
    reading(turn(T1, 'running', 2), [
      [3, 'text', 'Hello'],
      [4, 'text', ', world.'],
    ]),
  );
  await flush();
  const answer = () => turns()[0].querySelector('.turn-answer')!;
  expect(text(answer())).toBe('Codex Hello, world.');
  expect(text(turns()[0])).not.toContain('Thinking');

  ipc.world.turns.set(S, [turn(T1, 'completed', 3)]);
  ipc.last('managed_turn_read').resolve(
    reading(turn(T1, 'completed', 3), [
      [5, 'message_end'],
      [6, 'completed'],
    ]),
  );
  await flush();
  expect(text(turns()[0].querySelector('.turn-head .pill'))).toBe('Completed');
  expect(text(answer())).toBe('Codex Hello, world.');
  expect(button('Cancel turn')).toBeUndefined();
  // Ended: it is not read again, and another turn can be written.
  const reads = ipc.count('managed_turn_read');
  await flush();
  expect(ipc.count('managed_turn_read')).toBe(reads);
  expect(text(agent().querySelector('.composer-hint'))).toBe(
    'Enter to run · Shift+Enter for a new line',
  );

  // A second submission is a second turn, under the first.
  await run(ipc, 'And again.', T2);
  expect(ipc.count('managed_turn_start')).toBe(2);
  expect(ipc.last('managed_turn_start').request.intent_id).not.toBe(
    ipc.of('managed_turn_start')[0].request.intent_id,
  );
  expect(turns().map((t) => text(t.querySelector('.turn-prompt')))).toEqual([
    'You Say hello.',
    'You And again.',
  ]);
});

it('shows what Codex wrote as text and never as markup', async () => {
  const ipc = await opened();
  const hostile =
    '<img src=x onerror="window.pwned=1"><script>window.pwned=2</script>' +
    '<a href="javascript:window.pwned=3">x</a> **bold** [l](javascript:void(0))' +
    '<style>*{display:none}</style><iframe src="https://example.com"></iframe>';
  await run(ipc, hostile);
  ipc.last('managed_turn_read').resolve(
    reading(turn(T1, 'running', 2), [
      [1, 'text', hostile],
      [2, 'text', '\nline two'],
    ]),
  );
  await flush();
  const transcript = agent().querySelector('.agent-transcript')!;
  // Not one element came out of it: it is the characters that were written.
  expect(
    transcript.querySelectorAll('img, script, a, style, iframe, b, strong'),
  ).toHaveLength(0);
  expect(transcript.querySelector('.turn-answer .turn-text')!.textContent).toBe(
    `${hostile}\nline two`,
  );
  expect(transcript.querySelector('.turn-prompt .turn-text')!.textContent).toBe(
    hostile,
  );
  expect((window as unknown as { pwned?: number }).pwned).toBeUndefined();
  expect(transcript.innerHTML).toContain('&lt;img src=x');
});

it('says why a start failed, and offers the same request again only when it may be', async () => {
  const ipc = await opened();
  await type('Do it.');
  await press({ key: 'Enter' });
  ipc.last('managed_turn_start').reject({ code: 'unavailable', retry: true });
  await flush();
  const alert = () => agent().querySelector('.turn-alert')!;
  // An answer that did not come is an unknown outcome, not a refusal.
  expect(text(alert())).toMatch(/it is not known whether the turn started/);
  expect(alert().classList.contains('unknown')).toBe(true);
  expect(text(alert())).not.toMatch(/was not started/);
  expect(button('Run')!.disabled).toBe(true);
  // Retry is the same request, whole.
  await click(button('Retry'));
  expect(ipc.count('managed_turn_start')).toBe(2);
  expect(ipc.of('managed_turn_start')[1].request).toEqual(
    ipc.of('managed_turn_start')[0].request,
  );
  ipc.last('managed_turn_start').reject({ code: 'busy', retry: true });
  await flush();
  expect(text(alert())).toBe(
    'The turn was not started. Another turn is running. The authority runs one turn at a time. Retry Dismiss',
  );
  expect(alert().classList.contains('unknown')).toBe(false);
  // Dismissed, the input is the composer's again.
  await click(button('Dismiss'));
  expect(agent().querySelector('.turn-alert')).toBeNull();
  expect(box().value).toBe('Do it.');
  // A refusal that sending again cannot change offers no Retry.
  await press({ key: 'Enter' });
  ipc.last('managed_turn_start').reject({ code: 'changed', retry: false });
  await flush();
  expect(button('Retry')).toBeUndefined();
  expect(text(alert())).toMatch(/The session changed after it was shown/);
});

it('shows a failed and an indeterminate turn with the reason the authority recorded', async () => {
  const ipc = desktop();
  ipc.world.turns.set(S, [
    turn(T2, 'indeterminate', 3, {
      reason: 'dispatch_unknown',
      output: false,
    }),
    turn(T1, 'failed', 3, {
      reason: 'inference_isolation_violation',
      output: false,
    }),
  ]);
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  expect(turns().map((t) => text(t.querySelector('.turn-head .pill')))).toEqual(
    ['Failed', 'Indeterminate'],
  );
  expect(text(turns()[0])).toContain(
    'Codex tried to act outside what this session allows, and the authority stopped it.',
  );
  expect(text(turns()[1])).toContain(
    'The authority could not confirm whether Codex received the turn.',
  );
  // Turns this window did not start have no input to show, and say so.
  expect(text(turns()[0])).toContain('Wyss keeps no conversation text');
  expect(text(turns()[0])).toContain(
    'The authority no longer holds the output of this turn.',
  );
  expect(ipc.count('managed_turn_read')).toBe(0);
});

it('asks for a cancel and shows the turn as cancelling until the authority says it stopped', async () => {
  const ipc = await opened();
  await run(ipc, 'A long one.');
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[1, 'text', 'Working']]));
  await flush();
  await click(button('Cancel turn'));
  expect(ipc.of('managed_turn_cancel').map((c) => c.request)).toEqual([
    {
      intent_id: expect.stringMatching(UUID),
      turn_id: T1,
      expected_revision: 2,
    },
  ]);
  // Asked, not done. The button cannot ask twice and nothing says it was cancelled.
  expect(button('Cancelling…')!.disabled).toBe(true);
  expect(text(turns()[0])).toContain('Cancelling…');
  expect(text(turns()[0].querySelector('.turn-head .pill'))).toBe('Running');
  ipc.last('managed_turn_cancel').resolve({ id: T1 });
  await flush();
  expect(text(turns()[0].querySelector('.turn-head .pill'))).toBe('Running');
  expect(text(agent())).not.toMatch(/Interrupted|Cancelled/);
  expect(ipc.count('managed_turn_cancel')).toBe(1);

  ipc.world.turns.set(S, [turn(T1, 'interrupted', 3)]);
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'interrupted', 3), [[2, 'interrupted']]));
  await flush();
  expect(text(turns()[0].querySelector('.turn-head .pill'))).toBe(
    'Interrupted',
  );
  expect(text(turns()[0])).toContain('Stopped at your request.');
  expect(text(turns()[0])).not.toContain('Cancelling…');
  expect(button('Cancel turn')).toBeUndefined();
  expect(button('Cancelling…')).toBeUndefined();
  // What it had answered is still there.
  expect(text(turns()[0].querySelector('.turn-answer'))).toBe('Codex Working');
});

it('goes on with a turn while another view is shown, and leaves the terminal alone', async () => {
  const ipc = await opened();
  await run(ipc, 'Keep going.');
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[1, 'text', 'one ']]));
  await flush();
  const terminals = ipc.count('create_terminal');
  expect(terminals).toBe(1);

  await click(nav('Console'));
  expect(agent().hidden).toBe(true);
  // The turn is not cancelled and not started again: it is still being read.
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[2, 'text', 'two ']]));
  await flush();
  await click(nav('Sessions'));
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[3, 'text', 'three']]));
  await flush();
  await click(nav('Agents'));
  expect(agent().hidden).toBe(false);
  expect(text(turns()[0].querySelector('.turn-answer'))).toBe(
    'Codex one two three',
  );
  expect(text(turns()[0].querySelector('.turn-prompt'))).toBe(
    'You Keep going.',
  );
  expect(ipc.count('managed_turn_start')).toBe(1);
  expect(ipc.count('managed_turn_cancel')).toBe(0);
  // One reading at a time, each from where the last one ended.
  expect(ipc.of('managed_turn_read').map((c) => c.request.after)).toEqual([
    null,
    '1',
    '2',
    '3',
  ]);
  // The terminal was neither closed nor created again.
  expect(ipc.count('create_terminal')).toBe(terminals);
  expect(ipc.count('close_terminal')).toBe(0);
  // A draft survives the trip too.
  await type('next thing');
  await click(nav('Console'));
  await click(nav('Agents'));
  expect(box().value).toBe('next thing');
});

it('keeps two sessions as two transcripts, two drafts and two tabs', async () => {
  const ipc = await opened(1);
  await run(ipc, 'For rag-corp.');
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[1, 'text', 'RAG ANSWER']]));
  await flush();
  await type('rag draft');

  await openAgent(2);
  const tabs = () =>
    Array.from(agent().querySelectorAll<HTMLElement>('[role="tab"]'));
  expect(tabs().map(text)).toEqual(['rag-corp', 'other']);
  expect(tabs().map((t) => t.getAttribute('aria-selected'))).toEqual([
    'false',
    'true',
  ]);
  expect(text(agent().querySelector('.agent-session'))).toBe(
    'other Ready /home/dev/other Codex · Inference',
  );
  // Nothing of the first session is on screen.
  expect(turns()).toHaveLength(0);
  expect(text(agent())).not.toMatch(/RAG ANSWER|For rag-corp|rag draft/);
  expect(box().value).toBe('');
  // Its turn still runs, so this session cannot start one; the authority would refuse.
  await type('other draft');

  await click(tabs()[0]);
  expect(text(turns()[0].querySelector('.turn-answer'))).toBe(
    'Codex RAG ANSWER',
  );
  expect(box().value).toBe('rag draft');
  expect(text(agent())).not.toContain('other draft');
  // Closing a tab ends nothing at the authority.
  await click(
    agent().querySelector<HTMLElement>('[aria-label="Close other"]')!,
  );
  expect(tabs().map(text)).toEqual(['rag-corp']);
  expect(ipc.count('managed_turn_cancel')).toBe(0);
  expect(ipc.count('managed_turn_start')).toBe(1);
});

it('stops offering a turn when the authority goes away, and keeps what was read', async () => {
  const ipc = await opened();
  await run(ipc, 'Before the restart.');
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[1, 'text', 'partial']]));
  await flush();
  ipc.world.authority!({ ...views.waiting_absent, revision: 4 });
  await flush();
  expect(text(agent().querySelector('.sessions-status'))).toMatch(
    /What is shown was read before the connection was lost\./,
  );
  expect(text(turns()[0].querySelector('.turn-answer'))).toContain('partial');
  expect(button('Run')!.disabled).toBe(true);
  expect(button('Cancel turn')!.disabled).toBe(true);
  expect(text(agent().querySelector('.composer-hint'))).toMatch(
    /The authority is unavailable/,
  );
  await type('typed while offline');
  await press({ key: 'Enter' });
  expect(ipc.count('managed_turn_start')).toBe(1);
  expect(box().value).toBe('typed while offline');
});
