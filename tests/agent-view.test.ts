import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import type { TurnSummary } from '../src/lib/contracts/turns';
import { localTime } from '../src/lib/features/sessions/present';
import {
  cursor,
  historyPage,
  listing,
  reading,
  S,
  S2,
  T1,
  T2,
  turn,
} from './agent-harness';
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
    /** The sessions Sessions lists, when the test needs others than `page`. */
    page: undefined as typeof page | undefined,
    /** Where the list says older turns begin. */
    older: null as ReturnType<typeof cursor> | null,
    /** Where the list of a session other than `S` says older turns begin. */
    olderOf: new Map<string, ReturnType<typeof cursor>>(),
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
    if (command === 'managed_sessions_page') return world.page ?? page;
    if (command === 'managed_turns') {
      const session = request.session_id as typeof S;
      return listing(world.turns.get(session) ?? [], {
        session: {
          id: session,
          workspace:
            (world.page ?? page).sessions.find((s) => s.id === session)
              ?.workspace ??
            (session === S2 ? '/home/dev/other' : '/home/dev/rag-corp'),
        },
        older:
          world.olderOf.get(session) ?? (session === S ? world.older : null),
      });
    }
    // An ask for older turns waits for the test to answer it.
    if (command === 'managed_turns_older') return answer;
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

it('tells two sessions of one project apart in Sessions and in the Agent view', async () => {
  const ipc = desktop();
  const first = { ...bound, binding: 'bound_attached' };
  const second = {
    ...bound,
    id: S2,
    binding: 'bound_attached',
    position: '3',
    created_at: '2026-10-09T09:30:00Z',
    updated_at: '2026-10-09T09:30:00Z',
  };
  ipc.world.page = { ...page, sessions: [fresh, first, second] };
  component = mount(App, { target: document.body });
  await flush();
  await click(nav('Sessions'));
  const nameFirst = `rag-corp · ${localTime(first.created_at)}`;
  const nameSecond = `rag-corp · ${localTime(second.created_at)}`;
  expect(nameFirst).not.toBe(nameSecond);
  expect(
    Array.from(sessions().querySelectorAll('.session-name')).map(text),
  ).toEqual(['Session 33333333', nameFirst, nameSecond]);
  // The prompts that name a session name this one, not its twin.
  const rows = sessions().querySelectorAll<HTMLButtonElement>('.session-row');
  await click(rows[2]);
  expect(text(sessions().querySelector('.session-details h3'))).toBe(
    nameSecond,
  );

  await click(button('Open agent', sessions()));
  await openAgent(1);
  const tabs = Array.from(
    agent().querySelectorAll<HTMLElement>('[role="tab"]'),
  );
  expect(tabs.map(text)).toEqual([nameSecond, nameFirst]);
  expect(text(agent().querySelector('.agent-session h2'))).toBe(nameFirst);
  // Closing a tab names the one it closes.
  expect(
    agent().querySelector(`[aria-label="Close ${nameSecond}"]`),
  ).not.toBeNull();
});

// ---- Older turns -----------------------------------------------------------------------

const tid = (n: number) =>
  `${n.toString(16).padStart(8, '0')}-0000-4000-8000-000000000000` as typeof T1;
/** An ended turn of `S` that the authority no longer holds the output of, started at minute `n`. */
const old = (n: number) =>
  turn(tid(n), 'completed', 3, {
    output: false,
    started_at: `2026-10-09T10:${String(n % 60).padStart(2, '0')}:00Z`,
  });
const span = (high: number, low: number) =>
  Array.from({ length: high - low + 1 }, (_, i) => old(high - i));
const older = () => agent().querySelector<HTMLButtonElement>('.history-more');
const times = () =>
  Array.from(agent().querySelectorAll('.turn-head time')).map((t) =>
    t.getAttribute('datetime'),
  );
/** The application with `S` open, its newest twenty turns, and older ones beyond. */
async function withHistory() {
  const ipc = desktop();
  ipc.world.turns.set(S, span(100, 81));
  ipc.world.older = cursor(500);
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  return ipc;
}

it('offers older turns only where there are some, and says nothing where there are none', async () => {
  const ipc = desktop();
  ipc.world.turns.set(S, span(3, 1));
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  expect(turns()).toHaveLength(3);
  expect(older()).toBeNull();
  expect(text(agent())).not.toMatch(/older turns|start of this session/i);
});

it('loads older turns above the newest ones and says what is not kept, without inventing it', async () => {
  const ipc = await withHistory();
  expect(turns()).toHaveLength(20);
  expect(text(older())).toBe('Load older turns');

  await click(older()!);
  // The ask carries the session and the position the list gave, and nothing else.
  expect(ipc.of('managed_turns_older').map((c) => c.request)).toEqual([
    { session_id: S, stream_id: cursor(500).stream_id, before: '500' },
  ]);
  // While it is read: said so, and not asked twice.
  expect(text(agent())).toContain('Loading older turns…');
  expect(agent().querySelector('[role="status"]')).not.toBeNull();
  expect(older()).toBeNull();
  expect(turns()).toHaveLength(20);

  ipc
    .last('managed_turns_older')
    .resolve(historyPage(span(80, 61), cursor(300)));
  await flush();
  expect(turns()).toHaveLength(40);
  // Oldest first, the older page above the newest, each turn once.
  const shown = times();
  expect(shown[0]).toBe('2026-10-09T10:01:00Z');
  expect(shown[19]).toBe('2026-10-09T10:20:00Z');
  expect(shown[20]).toBe('2026-10-09T10:21:00Z');
  expect(new Set(shown).size).toBe(40);
  // An old turn is shown as the authority holds it: it is over, and its output is gone.
  const first = turns()[0];
  expect(text(first.querySelector('.turn-head .pill'))).toBe('Completed');
  expect(text(first.querySelector('.turn-answer'))).toBe(
    'Codex The authority no longer holds the output of this turn.',
  );
  expect(text(first)).toContain('Wyss keeps no conversation text');
  expect(first.querySelector('.turn-text')).toBeNull();
  // More behind it, so it can be asked again, from where this one said.
  expect(text(older())).toBe('Load older turns');
  await click(older()!);
  expect(ipc.last('managed_turns_older').request).toEqual({
    session_id: S,
    stream_id: cursor(300).stream_id,
    before: '300',
  });
  ipc.last('managed_turns_older').resolve(historyPage(span(60, 41), null));
  await flush();
  expect(turns()).toHaveLength(60);
  // The start of history: said, and nothing left to ask for.
  expect(older()).toBeNull();
  expect(text(agent())).toContain(
    "This is the start of this session's history.",
  );
  // Nothing here started, cancelled or decided anything.
  expect(ipc.count('managed_turn_start')).toBe(0);
  expect(ipc.count('managed_turn_cancel')).toBe(0);
  expect(ipc.count('managed_approval_approve')).toBe(0);
});

it('keeps the reader where they were when older turns are inserted above them', async () => {
  const ipc = await withHistory();
  const log = agent().querySelector<HTMLElement>('.agent-transcript')!;
  // jsdom lays nothing out: give the transcript the sizes a reader scrolled up would see.
  let height = 1000;
  Object.defineProperty(log, 'scrollHeight', { get: () => height });
  Object.defineProperty(log, 'clientHeight', { value: 300 });
  log.scrollTop = 100;
  log.dispatchEvent(new Event('scroll'));
  await flush();

  await click(older()!);
  // Four hundred pixels of older turns arrive above what the reader was looking at.
  height = 1400;
  ipc
    .last('managed_turns_older')
    .resolve(historyPage(span(80, 61), cursor(300)));
  await flush();
  await flush();
  // The same turns are under their eyes: the distance to the end did not change.
  expect(log.scrollTop).toBe(1400 - (1000 - 100));
});

it('says when a stretch of history held no turn and lets the reader keep looking', async () => {
  const ipc = await withHistory();
  await click(older()!);
  ipc.last('managed_turns_older').resolve(historyPage([], cursor(250)));
  await flush();
  expect(turns()).toHaveLength(20);
  expect(text(agent())).toContain(
    'No turns in the part of the history just searched',
  );
  expect(text(older())).toBe('Keep looking');
  await click(older()!);
  expect(ipc.last('managed_turns_older').request.before).toBe('250');
  ipc.last('managed_turns_older').resolve(historyPage(span(40, 31), null));
  await flush();
  expect(turns()).toHaveLength(30);
  expect(text(agent())).not.toContain('just searched');
});

it('says why older turns could not be loaded, keeps what is shown and asks again from the same place', async () => {
  const ipc = await withHistory();
  await click(older()!);
  ipc.last('managed_turns_older').reject({ code: 'unavailable', retry: true });
  await flush();
  const alert = agent().querySelector('.turn-alert')!;
  expect(text(alert)).toContain(
    'Older turns could not be loaded: the authority did not answer',
  );
  expect(turns()).toHaveLength(20);
  await click(button('Try again', alert as HTMLElement));
  expect(ipc.count('managed_turns_older')).toBe(2);
  expect(ipc.last('managed_turns_older').request.before).toBe('500');
  expect(agent().querySelector('.turn-alert')).toBeNull();
  ipc
    .last('managed_turns_older')
    .resolve(historyPage(span(80, 61), cursor(300)));
  await flush();
  expect(turns()).toHaveLength(40);
});

it('reloads the newest turns when the history changed under the place it had reached', async () => {
  const ipc = await withHistory();
  await click(older()!);
  ipc
    .last('managed_turns_older')
    .resolve(historyPage(span(80, 61), cursor(300)));
  await flush();
  await click(older()!);
  ipc.last('managed_turns_older').reject({ code: 'changed', retry: false });
  await flush();
  const alert = agent().querySelector('.turn-alert')!;
  expect(text(alert)).toContain('The history changed');
  // The only way on is to start again from the newest: there is no retry of a bad place.
  expect(button('Try again', alert as HTMLElement)).toBeUndefined();
  expect(turns()).toHaveLength(40);

  ipc.world.older = cursor(900, '77777777-7777-4777-8777-777777777777');
  const lists = ipc.count('managed_turns');
  await click(button('Reload turns', alert as HTMLElement));
  expect(ipc.count('managed_turns')).toBe(lists + 1);
  expect(turns()).toHaveLength(20);
  expect(agent().querySelector('.turn-alert')).toBeNull();
  await click(older()!);
  expect(ipc.last('managed_turns_older').request).toEqual({
    session_id: S,
    stream_id: '77777777-7777-4777-8777-777777777777',
    before: '900',
  });
});

it('keeps a running turn followed while older turns are asked for', async () => {
  const ipc = desktop();
  ipc.world.turns.set(S, [turn(T1, 'running', 2), ...span(99, 81)]);
  ipc.world.older = cursor(500);
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  // The running turn is followed from the first moment.
  expect(ipc.of('managed_turn_read').map((c) => c.request)).toEqual([
    { turn_id: T1, after: null, wait: true },
  ]);
  await click(older()!);
  expect(ipc.count('managed_turns_older')).toBe(1);
  // Its text arrives while the ask is still waiting, and the ask did not delay it.
  ipc
    .last('managed_turn_read')
    .resolve(reading(turn(T1, 'running', 2), [[1, 'text', 'STILL GOING']]));
  await flush();
  expect(text(turns().at(-1)!.querySelector('.turn-answer'))).toContain(
    'STILL GOING',
  );
  expect(text(agent())).toContain('Loading older turns…');
  expect(ipc.count('managed_turn_read')).toBe(2);
  ipc
    .last('managed_turns_older')
    .resolve(historyPage(span(80, 61), cursor(300)));
  await flush();
  expect(turns()).toHaveLength(40);
  expect(text(turns().at(-1)!.querySelector('.turn-answer'))).toContain(
    'STILL GOING',
  );
});

it('does not pull a reader who scrolled up back to the end when new output arrives', async () => {
  const ipc = desktop();
  ipc.world.turns.set(S, [turn(T1, 'running', 2), ...span(99, 90)]);
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  const log = agent().querySelector<HTMLElement>('.agent-transcript')!;
  Object.defineProperty(log, 'scrollHeight', { get: () => 1000 });
  Object.defineProperty(log, 'clientHeight', { value: 300 });
  // The reader scrolls up to read, far from the end.
  log.scrollTop = 100;
  log.dispatchEvent(new Event('scroll'));
  await flush();
  for (const [at, chunk] of [
    [1, 'FIRST '],
    [2, 'SECOND '],
  ] as const) {
    ipc
      .last('managed_turn_read')
      .resolve(reading(turn(T1, 'running', 2), [[at, 'text', chunk]]));
    await flush();
    await flush();
  }
  // Output arrived twice, and the reader is still where they were.
  expect(text(turns().at(-1)!.querySelector('.turn-answer'))).toContain(
    'FIRST SECOND',
  );
  expect(log.scrollTop).toBe(100);
});

it('opens another session at the end of its transcript, wherever the reader was in the last', async () => {
  const ipc = desktop();
  ipc.world.turns.set(S, span(99, 90));
  ipc.world.turns.set(S2, span(89, 80));
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  const log = agent().querySelector<HTMLElement>('.agent-transcript')!;
  Object.defineProperty(log, 'scrollHeight', { get: () => 1000 });
  Object.defineProperty(log, 'clientHeight', { value: 300 });
  log.scrollTop = 100;
  log.dispatchEvent(new Event('scroll'));
  await flush();
  await openAgent(2);
  await flush();
  expect(log.scrollTop).toBe(1000);
});

// ---- Names and older turns, in one application ------------------------------------------

/** Two Ready sessions of one project, which the Sessions page names apart. */
function twins() {
  const first = { ...bound, binding: 'bound_attached' };
  const second = {
    ...bound,
    id: S2,
    binding: 'bound_attached',
    position: '3',
    created_at: '2026-10-09T09:30:00Z',
    updated_at: '2026-10-09T09:30:00Z',
  };
  return {
    first,
    second,
    nameFirst: `rag-corp · ${localTime(first.created_at)}`,
    nameSecond: `rag-corp · ${localTime(second.created_at)}`,
  };
}
/** An ended turn of the second session, started on another day than the first's. */
const oldTwo = (n: number) =>
  turn(tid(n), 'completed', 3, {
    session: S2,
    output: false,
    started_at: `2026-10-10T10:${String(n % 60).padStart(2, '0')}:00Z`,
  });
const spanTwo = (high: number, low: number) =>
  Array.from({ length: high - low + 1 }, (_, i) => oldTwo(high - i));
const tabs = () =>
  Array.from(agent().querySelectorAll<HTMLElement>('[role="tab"]'));
const day = (stamp: string | null) => stamp?.slice(0, 10);

it('keeps two sessions of one project apart while each loads its own older turns', async () => {
  const { first, second, nameFirst, nameSecond } = twins();
  const ipc = desktop();
  ipc.world.page = { ...page, sessions: [fresh, first, second] };
  ipc.world.turns.set(S, span(100, 81));
  ipc.world.turns.set(S2, spanTwo(60, 41));
  ipc.world.older = cursor(500);
  ipc.world.olderOf.set(S2, cursor(200));
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  await openAgent(2);
  expect(tabs().map(text)).toEqual([nameFirst, nameSecond]);
  expect(text(agent().querySelector('.agent-session h2'))).toBe(nameSecond);
  expect(turns()).toHaveLength(20);

  // The second session asks for its own older turns, from the place its own list gave.
  await click(older()!);
  expect(ipc.of('managed_turns_older').map((c) => c.request)).toEqual([
    { session_id: S2, stream_id: cursor(200).stream_id, before: '200' },
  ]);
  ipc
    .last('managed_turns_older')
    .resolve(historyPage(spanTwo(40, 21), cursor(100), S2));
  await flush();
  expect(turns()).toHaveLength(40);
  expect(new Set(times().map(day))).toEqual(new Set(['2026-10-10']));
  // Loading history does not rename anything.
  expect(tabs().map(text)).toEqual([nameFirst, nameSecond]);
  expect(text(agent().querySelector('.agent-session h2'))).toBe(nameSecond);

  // The first session has its own history, and none of the second's reaches it.
  await click(tabs()[0]);
  expect(text(agent().querySelector('.agent-session h2'))).toBe(nameFirst);
  expect(turns()).toHaveLength(20);
  expect(new Set(times().map(day))).toEqual(new Set(['2026-10-09']));
  await click(older()!);
  expect(ipc.last('managed_turns_older').request).toEqual({
    session_id: S,
    stream_id: cursor(500).stream_id,
    before: '500',
  });
  ipc.last('managed_turns_older').resolve(historyPage(span(80, 61), null));
  await flush();
  expect(turns()).toHaveLength(40);
  expect(new Set(times().map(day))).toEqual(new Set(['2026-10-09']));
  expect(text(agent())).toContain(
    "This is the start of this session's history.",
  );

  // Back in the second, what it loaded is still there, and it can still go further back.
  await click(tabs()[1]);
  expect(turns()).toHaveLength(40);
  expect(new Set(times().map(day))).toEqual(new Set(['2026-10-10']));
  expect(text(older())).toBe('Load older turns');
  expect(text(agent())).not.toContain('start of this session');
  expect(ipc.count('managed_turns_older')).toBe(2);
  // Reading history started, cancelled and decided nothing.
  expect(ipc.count('managed_turn_start')).toBe(0);
  expect(ipc.count('managed_turn_cancel')).toBe(0);
  expect(ipc.count('managed_approval_approve')).toBe(0);
});

it('refuses a page of the twin session, and shows neither its turns nor a new name', async () => {
  const { first, second, nameFirst, nameSecond } = twins();
  const ipc = desktop();
  ipc.world.page = { ...page, sessions: [fresh, first, second] };
  ipc.world.turns.set(S, span(100, 81));
  ipc.world.turns.set(S2, spanTwo(60, 41));
  ipc.world.older = cursor(500);
  ipc.world.olderOf.set(S2, cursor(200));
  component = mount(App, { target: document.body });
  await flush();
  await openAgent(1);
  await openAgent(2);

  await click(older()!);
  expect(ipc.last('managed_turns_older').request.session_id).toBe(S2);
  // The authority answers with the first session's page, turns and all.
  ipc.last('managed_turns_older').resolve(historyPage(span(80, 61), null, S));
  await flush();
  expect(turns()).toHaveLength(20);
  expect(new Set(times().map(day))).toEqual(new Set(['2026-10-10']));
  expect(text(agent().querySelector('.turn-alert'))).toContain(
    'Nothing is shown rather than something unverified.',
  );
  expect(tabs().map(text)).toEqual([nameFirst, nameSecond]);
  // Neither tab took anything from it.
  await click(tabs()[0]);
  expect(turns()).toHaveLength(20);
  expect(agent().querySelector('.turn-alert')).toBeNull();
  expect(text(older())).toBe('Load older turns');
});
