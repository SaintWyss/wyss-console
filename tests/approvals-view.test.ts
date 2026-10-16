import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import type { TurnSummary } from '../src/lib/contracts/turns';
import { localTime } from '../src/lib/features/sessions/present';
import { listing, reading, S, S2, T1, T2, turn } from './agent-harness';
import approvals from './fixtures/approval-list-v1.json';
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
const [, bound] = pages.loaded.sessions;
const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const C = approvals.command.approval.id;
const F = approvals.file.approval.id;
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
/** One session, attached in Local Tools: the runtime serves its workspace. */
const page = {
  ...pages.loaded,
  runtime: { mode: 'local_tools', workspace: bound.workspace },
  sessions: [{ ...bound, binding: 'bound_attached', mode: 'local_tools' }],
};
const none = approvals.empty;
const pendingList = (...pending: unknown[]) => ({
  ...approvals.empty,
  head: '10',
  pending,
});

interface Call {
  command: string;
  request: Record<string, unknown>;
  resolve: (value: unknown) => void;
  reject: (failure: unknown) => void;
}
/**
 * A desktop whose terminal works and whose one session runs in Local Tools. Lists are
 * answered from what the test says the authority holds; turn calls and decisions wait
 * for the test to answer them.
 */
function desktop() {
  const calls: Call[] = [];
  const world = {
    turns: [] as TurnSummary[],
    approvals: none as unknown,
    /** What a read of one approval answers. */
    detail: (request: Record<string, unknown>): unknown => {
      void request;
      return approvals.command;
    },
    authority: undefined as ((view: unknown) => void) | undefined,
    /** The sessions Sessions lists, when the test needs others than `page`. */
    sessions: undefined as typeof page.sessions | undefined,
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
    if (command === 'managed_sessions_page')
      return { ...page, sessions: world.sessions ?? page.sessions };
    if (command === 'managed_turns')
      return listing(world.turns, { session: { mode: 'local_tools' } });
    if (command === 'managed_approvals') return world.approvals;
    if (command === 'managed_approval_read') return world.detail(request);
    if (
      command.startsWith('managed_turn_') ||
      command.startsWith('managed_approval_')
    )
      return answer;
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
    /** The authority's durable head moves: what the views read is behind. */
    async advance(head: string, revision: number) {
      world.authority!(ready(head, revision));
      await flush();
    },
  };
}
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label))!;
const agent = () => document.querySelector<HTMLElement>('.agent-feature')!;
const sessions = () =>
  document.querySelector<HTMLElement>('.sessions-feature')!;
const approvalsView = () =>
  document.querySelector<HTMLElement>('.approvals-feature')!;
const button = (label: string, root: ParentNode) =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  );
const cards = (root: ParentNode = document) =>
  Array.from(root.querySelectorAll<HTMLElement>('.approval-card'));
async function click(target: HTMLElement | undefined) {
  expect(target).toBeDefined();
  target!.click();
  await flush();
}
/** The application with its Local Tools session open in the Agent view, a turn running. */
async function running(
  setup: (ipc: ReturnType<typeof desktop>) => void = () => undefined,
) {
  const ipc = desktop();
  setup(ipc);
  component = mount(App, { target: document.body });
  await flush();
  await click(nav('Sessions'));
  await click(sessions().querySelector<HTMLButtonElement>('.session-row')!);
  await click(button('Open agent', sessions()));
  const box = agent().querySelector<HTMLTextAreaElement>('textarea')!;
  box.value = 'List the files.';
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  box.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    }),
  );
  await flush();
  ipc.world.turns = [turn(T1, 'running', 2)];
  ipc.last('managed_turn_start').resolve({ id: T1 });
  await flush();
  // A turn is being followed: approvals are read from now on, and were not before.
  expect(ipc.count('managed_approvals')).toBe(1);
  return ipc;
}
/** The running turn reports that it waits on `approval`, which the authority now lists. */
async function asks(
  ipc: ReturnType<typeof desktop>,
  detail: {
    approval: { id: string; kind: string; expires_at: string };
  },
  read: (request: Record<string, unknown>) => unknown = () => detail,
) {
  ipc.world.approvals = pendingList(detail);
  ipc.world.detail = read;
  ipc.last('managed_turn_read').resolve(
    reading(
      turn(T1, 'running', 2),
      [
        [1, 'started'],
        [2, 'approval_required'],
      ],
      {
        approval: {
          id: detail.approval.id as never,
          kind: detail.approval.kind as never,
          expires_at: detail.approval.expires_at,
        },
      },
    ),
  );
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

it('opens Approvals from the sidebar and reads nothing for it before that', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  expect(nav('Approvals').disabled).toBe(false);
  expect(approvalsView().hidden).toBe(true);
  // On the Console, with no turn followed, approvals are not read at all.
  expect(ipc.count('managed_approvals')).toBe(0);
  await click(nav('Sessions'));
  await click(nav('Agents'));
  expect(ipc.count('managed_approvals')).toBe(0);

  await click(nav('Approvals'));
  expect(approvalsView().hidden).toBe(false);
  expect(document.querySelector('.context-title')?.textContent).toBe(
    'Approvals',
  );
  expect(ipc.of('managed_approvals').map((c) => c.request)).toEqual([{}]);
  expect(text(approvalsView())).toContain('0 waiting for a decision');
  expect(text(approvalsView())).toContain('Nothing is waiting for a decision.');
  expect(cards()).toHaveLength(0);
  expect(document.querySelector('.nav-tag.waiting')).toBeNull();
  // Nothing changes at the authority: nothing is read again.
  await flush();
  expect(ipc.count('managed_approvals')).toBe(1);

  // An approval that ended is shown as what it ended as, with nothing to decide.
  ipc.world.approvals = {
    ...approvals.empty,
    head: '10',
    recent: approvals.recent,
  };
  await ipc.advance('10', 4);
  expect(ipc.count('managed_approvals')).toBe(2);
  const rows = Array.from(approvalsView().querySelectorAll('.approval-row'));
  expect(rows.map((r) => text(r.querySelector('.pill')))).toEqual([
    'Approved',
    'Expired',
  ]);
  expect(text(rows[1])).toContain('File change');
  expect(text(rows[1])).toContain('Nobody decided it in time.');
  expect(approvalsView().querySelectorAll('.approval-card')).toHaveLength(0);
  expect(button('Approve', approvalsView())).toBeUndefined();

  // The authority goes away: nothing is shown as pending, and nothing can be decided.
  ipc.world.authority!({ ...views.waiting_absent, revision: 5 });
  await flush();
  expect(text(approvalsView())).toContain('The authority is unavailable');
  expect(text(approvalsView())).toContain(
    'Nothing is approved or denied in the meantime.',
  );
  expect(approvalsView().querySelectorAll('.approval-row')).toHaveLength(0);
});

it('shows an approval under the turn that waits on it and in Approvals, and approves it once', async () => {
  const ipc = await running();
  // The session is attached in Local Tools, and says so.
  expect(text(agent().querySelector('.agent-who'))).toBe('Codex · Local Tools');
  await asks(ipc, approvals.command);

  // Under its turn: what Codex wants to run, where, and the two things one can answer.
  const inline = cards(agent());
  expect(inline).toHaveLength(1);
  expect(text(inline[0].querySelector('.approval-title'))).toBe(
    'Codex wants to run a command',
  );
  expect(inline[0].querySelector('.approval-code')!.textContent).toBe(
    "/usr/bin/bash -lc 'git status'",
  );
  expect(text(inline[0].querySelector('.approval-fields'))).toContain(
    'Directory /home/dev/rag-corp',
  );
  expect(
    Array.from(inline[0].querySelectorAll('button')).map((b) => text(b)),
  ).toEqual(['Approve', 'Deny']);
  // The turn is what the authority says it is: running.
  expect(text(agent().querySelector('.turn-head .pill'))).toBe('Running');
  // The sidebar says something waits, wherever the user is.
  expect(text(document.querySelector('.nav-tag.waiting'))).toBe('1');
  // No handle, fingerprint or lease is on screen.
  expect(text(document.body)).not.toMatch(
    new RegExp(`${C}|fingerprint|lease|provider`, 'i'),
  );

  // The same approval in Approvals, named by its session and turn.
  await click(nav('Approvals'));
  const listed = cards(approvalsView());
  expect(listed).toHaveLength(1);
  expect(text(listed[0].querySelector('.approval-context'))).toBe(
    `rag-corp · session ${S} · turn ${T1.slice(0, 8)}`,
  );
  // Under its turn it carries the same identity, without the name the turn already has.
  expect(text(inline[0].querySelector('.approval-context'))).toBe(
    `session ${S} · turn ${T1.slice(0, 8)}`,
  );
  expect(text(approvalsView())).toContain('1 waiting for a decision');
  // One reader: showing the page asked for no second list.
  const lists = ipc.count('managed_approvals');

  // Approve, from the page. One request: the approval, its revision, the user's action.
  await click(button('Approve', listed[0]));
  expect(ipc.of('managed_approval_approve').map((c) => c.request)).toEqual([
    {
      intent_id: expect.stringMatching(UUID),
      approval_id: C,
      expected_revision: 1,
    },
  ]);
  // Pending: neither button can be pressed again, here or under the turn.
  for (const card of cards()) {
    expect(button('Approve', card)!.disabled).toBe(true);
    expect(button('Deny', card)!.disabled).toBe(true);
    expect(text(card)).toContain('Asking the authority to approve it…');
  }
  button('Approve', listed[0])!.click();
  button('Deny', listed[0])!.click();
  await flush();
  expect(ipc.count('managed_approval_approve')).toBe(1);
  expect(ipc.count('managed_approval_deny')).toBe(0);
  // Nothing says "approved" before the authority does.
  expect(text(document.body)).not.toContain('Approved');

  // The authority committed it: the approval is read again and is no longer pending.
  ipc.world.approvals = {
    ...approvals.empty,
    head: '11',
    recent: [{ ...approvals.recent[0], id: C, turn: T1 }],
  };
  ipc.last('managed_approval_approve').resolve({ id: C, state: 'accepted' });
  await flush();
  expect(ipc.count('managed_approvals')).toBe(lists + 1);
  expect(cards()).toHaveLength(0);
  expect(document.querySelector('.nav-tag.waiting')).toBeNull();
  expect(text(approvalsView().querySelector('.approval-row .pill'))).toBe(
    'Approved',
  );

  // The same turn goes on: its stream says what happened, and no turn was started.
  await click(nav('Agents'));
  ipc.world.turns = [turn(T1, 'completed', 3)];
  ipc.last('managed_turn_read').resolve(
    reading(turn(T1, 'completed', 3), [
      [3, 'approval_resolved', 'accepted'],
      [4, 'command_started'],
      [5, 'command_output'],
      [6, 'command_completed'],
      [7, 'text', 'There are three files.'],
      [8, 'completed'],
    ]),
  );
  await flush();
  expect(
    Array.from(agent().querySelectorAll('.turn-activity')).map(text),
  ).toEqual([
    'Approval requested',
    'Approval accepted',
    'Command started',
    'Command output',
    'Command finished',
  ]);
  expect(text(agent().querySelector('.turn-answer'))).toContain(
    'There are three files.',
  );
  expect(text(agent().querySelector('.turn-head .pill'))).toBe('Completed');
  expect(ipc.count('managed_turn_start')).toBe(1);
  expect(ipc.count('managed_turn_cancel')).toBe(0);
});

it('denies one approval, and settles a decision whose answer was lost with the same request', async () => {
  const ipc = await running();
  await asks(ipc, approvals.command);
  const card = () => cards(agent())[0];
  await click(button('Deny', card()));
  expect(ipc.of('managed_approval_deny').map((c) => c.request)).toEqual([
    {
      intent_id: expect.stringMatching(UUID),
      approval_id: C,
      expected_revision: 1,
    },
  ]);
  expect(ipc.count('managed_approval_approve')).toBe(0);
  expect(text(card())).toContain('Asking the authority to deny it…');

  // No answer came. Not known to be taken: a caution, never a refusal.
  ipc
    .last('managed_approval_deny')
    .reject({ code: 'unavailable', retry: true });
  await flush();
  const alert = () => card().querySelector('.turn-alert')!;
  expect(text(alert())).toMatch(
    /it is not known whether the decision was taken/,
  );
  expect(alert().classList.contains('unknown')).toBe(true);
  // Neither decision can be sent as a new one while that one is unsettled.
  expect(button('Approve', card())!.disabled).toBe(true);
  expect(button('Deny', card())!.disabled).toBe(true);
  // Retry is the same request, whole.
  await click(button('Retry', card()));
  expect(ipc.count('managed_approval_deny')).toBe(2);
  expect(ipc.of('managed_approval_deny')[1].request).toEqual(
    ipc.of('managed_approval_deny')[0].request,
  );
  expect(ipc.count('managed_approval_approve')).toBe(0);

  // A refusal is said as one, and the approval is read again.
  ipc.last('managed_approval_deny').reject({ code: 'changed', retry: false });
  await flush();
  expect(text(alert())).toBe(
    'It was not denied. The approval changed after it was shown. It has been read again. Dismiss',
  );
  expect(alert().classList.contains('unknown')).toBe(false);
  expect(button('Retry', card())).toBeUndefined();
  // Read again and still there: the user lets go of it and may decide again.
  await click(button('Dismiss', card()));
  expect(card().querySelector('.turn-alert')).toBeNull();
  expect(button('Deny', card())!.disabled).toBe(false);
});

it('shows a file change as text, says when it is not whole, and approves only the whole of it', async () => {
  const ipc = await running();
  const hostile =
    '+<img src=x onerror="window.pwned=1"><script>window.pwned=2</script>\n';
  const first = {
    ...approvals.file,
    file: {
      ...approvals.file.file,
      chunk: hostile,
      bytes: hostile.length + 6,
      next: hostile.length,
    },
  };
  const second = {
    ...approvals.file,
    file: {
      ...approvals.file.file,
      index: 1,
      path: '/home/dev/rag-corp/README.md',
      kind: 'add',
      chunk: '+hello\n',
      bytes: 7,
    },
  };
  await asks(ipc, first, (request) =>
    request.file === 1
      ? second
      : request.offset === hostile.length
        ? {
            ...first,
            file: {
              ...first.file,
              offset: hostile.length,
              chunk: '+more\n',
              next: null,
            },
          }
        : first,
  );
  await flush();
  const card = () => cards(agent())[0];
  expect(text(card().querySelector('.approval-title'))).toBe(
    'Codex wants to change 2 files',
  );
  // Both files are named, with what would happen to each.
  const files = () => Array.from(card().querySelectorAll('.approval-file'));
  expect(files().map((f) => text(f.querySelector('.file-head')))).toEqual([
    'Modify /home/dev/rag-corp/src/parser.rs',
    'Add /home/dev/rag-corp/README.md',
  ]);
  // The diff is the characters that were written: not one element came out of it.
  expect(card().querySelectorAll('img, script, a, iframe')).toHaveLength(0);
  expect(files()[0].querySelector('.approval-code')!.textContent).toBe(hostile);
  expect((window as unknown as { pwned?: number }).pwned).toBeUndefined();
  // Not all of the first diff is shown, and that is said, never hidden.
  expect(text(files()[0])).toMatch(/Preview truncated: .* of .* bytes shown\./);
  expect(button('Approve', card())!.disabled).toBe(true);
  expect(text(card())).toContain('Read the whole change to approve it.');
  // Deny needs nothing more to be read.
  expect(button('Deny', card())!.disabled).toBe(false);

  await click(button('Show the rest', files()[0]));
  expect(files()[0].querySelector('.approval-code')!.textContent).toBe(
    `${hostile}+more\n`,
  );
  expect(text(files()[0])).not.toContain('Preview truncated');
  expect(button('Approve', card())!.disabled).toBe(false);
  expect(text(card())).not.toContain('Read the whole change');
  await click(button('Approve', card()));
  expect(ipc.last('managed_approval_approve').request).toEqual({
    intent_id: expect.stringMatching(UUID),
    approval_id: F,
    expected_revision: 1,
  });
  // The reads that showed it were of this approval, by file and by page.
  expect(
    ipc
      .of('managed_approval_read')
      .map((c) => [c.request.file, c.request.offset]),
  ).toEqual([
    [1, 0],
    [0, hostile.length],
  ]);
});

it('offers nothing beyond one decision for one approval', async () => {
  const ipc = await running();
  await asks(ipc, approvals.command);
  await click(nav('Approvals'));
  // Every control of both places an approval is shown.
  const labels = cards().flatMap((card) =>
    Array.from(card.querySelectorAll('button, a, input, select')).map((n) =>
      text(n),
    ),
  );
  expect(labels).toEqual(['Approve', 'Deny', 'Approve', 'Deny']);
  expect(text(document.body)).not.toMatch(
    /always allow|allow always|remember|trust this|for this session|edit command|add rule/i,
  );
  // And every command the window sent: reads, a turn, and nothing that decides by itself.
  const sent = [...new Set(ipc.calls.map((c) => c.command))];
  expect(sent.filter((c) => /approv/.test(c)).sort()).toEqual([
    'managed_approvals',
  ]);
  expect(ipc.count('managed_approval_approve')).toBe(0);
  expect(ipc.count('managed_approval_deny')).toBe(0);
  // Left alone, through changes at the authority: still nothing is answered.
  await ipc.advance('12', 4);
  await ipc.advance('13', 5);
  expect(ipc.count('managed_approval_approve')).toBe(0);
  expect(ipc.count('managed_approval_deny')).toBe(0);
});

it('never shows the approval of another turn under a turn, nor a command as markup', async () => {
  const ipc = await running();
  const hostile =
    '/usr/bin/bash -lc \'echo <img src=x onerror="window.pwned=1"><b>x</b>\'';
  // The turn says it waits on one approval. What the authority lists as pending is
  // another one, of another turn.
  const theirs = {
    ...approvals.command,
    approval: {
      ...approvals.command.approval,
      id: F,
      turn: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
    },
    command: { ...approvals.command.command, command: hostile },
  };
  ipc.world.approvals = pendingList(theirs);
  ipc.last('managed_turn_read').resolve(
    reading(turn(T1, 'running', 2), [[1, 'approval_required']], {
      approval: {
        id: C as never,
        kind: 'command_execution',
        expires_at: approvals.command.approval.expires_at,
      },
    }),
  );
  await flush();
  // Under this turn: that it waits, and nothing of the other turn's action.
  expect(cards(agent())).toHaveLength(0);
  expect(text(agent().querySelector('.turn-approval'))).toMatch(
    /^Waiting for approval Codex asked to run a command\./,
  );
  expect(text(agent())).not.toContain('echo');
  expect(button('Approve', agent())).toBeUndefined();
  // In Approvals it is shown as what it is: another turn's, as text.
  await click(nav('Approvals'));
  const [card] = cards(approvalsView());
  expect(text(card.querySelector('.approval-context'))).toBe(
    `rag-corp · session ${S} · turn bbbbbbbb`,
  );
  expect(card.querySelector('.approval-code')!.textContent).toBe(hostile);
  expect(card.querySelectorAll('img, b, script')).toHaveLength(0);
  expect((window as unknown as { pwned?: number }).pwned).toBeUndefined();
});

it('names the session an approval belongs to so a twin in the same folder is not mistaken for it', async () => {
  const mine = { ...bound, binding: 'bound_attached', mode: 'local_tools' };
  const twin = {
    ...mine,
    id: '66666666-6666-4666-8666-666666666666',
    position: '3',
    created_at: '2026-10-09T09:30:00Z',
    updated_at: '2026-10-09T09:30:00Z',
  };
  const ipc = await running((desktop) => {
    desktop.world.sessions = [mine, twin];
  });
  await asks(ipc, approvals.command);
  await click(nav('Approvals'));
  const listed = cards(approvalsView());
  expect(listed).toHaveLength(1);
  // Two sessions share the folder: the name says which one is asking, not just the folder.
  expect(text(listed[0].querySelector('.approval-context'))).toBe(
    `rag-corp · ${localTime(mine.created_at)} · session ${S} · turn ${T1.slice(0, 8)}`,
  );
});

/** A second pending approval, of another session in the very same directory. */
const A2 = 'eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee';
const ofTwin = {
  ...approvals.command,
  approval: {
    ...approvals.command.approval,
    id: A2,
    session: S2,
    turn: T2,
  },
};

it('says whose action it is by the session identity when no Sessions page was ever read', async () => {
  const ipc = desktop();
  // Two managed sessions in one directory, each with an action waiting.
  ipc.world.approvals = pendingList(approvals.command, ofTwin);
  component = mount(App, { target: document.body });
  await flush();
  await click(nav('Approvals'));

  // Nothing was read to name them: the identity is the approval's own.
  expect(ipc.count('managed_sessions_page')).toBe(0);
  const listed = cards(approvalsView());
  expect(listed).toHaveLength(2);
  const contexts = listed.map((card) =>
    text(card.querySelector('.approval-context')),
  );
  // The same name twice, and two identities that cannot be mistaken for each other.
  expect(contexts).toEqual([
    `rag-corp · session ${S} · turn ${T1.slice(0, 8)}`,
    `rag-corp · session ${S2} · turn ${T2.slice(0, 8)}`,
  ]);
  expect(S).not.toBe(S2);
  expect(text(document.querySelector('.nav-tag.waiting'))).toBe('2');

  // The answer goes to the approval of the card that was pressed, and to no other.
  await click(button('Deny', listed[1]));
  expect(ipc.of('managed_approval_deny').map((c) => c.request)).toEqual([
    {
      intent_id: expect.stringMatching(UUID),
      approval_id: A2,
      expected_revision: 1,
    },
  ]);
  expect(ipc.count('managed_approval_approve')).toBe(0);
  // Still without a Sessions page.
  expect(ipc.count('managed_sessions_page')).toBe(0);
});

it('keeps an approval with the session it came from once the Sessions page is read', async () => {
  const ipc = desktop();
  ipc.world.approvals = pendingList(approvals.command, ofTwin);
  component = mount(App, { target: document.body });
  await flush();
  await click(nav('Approvals'));
  const before = cards(approvalsView()).map((card) =>
    text(card.querySelector('.approval-context')),
  );
  // Sessions lists only one of the two: the other is beyond what that page holds.
  await click(nav('Sessions'));
  await click(nav('Approvals'));
  const after = cards(approvalsView()).map((card) =>
    text(card.querySelector('.approval-context')),
  );
  expect(after).toHaveLength(2);
  // The identity of each did not change. Only a name may have gained words.
  for (const [index, id] of [S, S2].entries()) {
    expect(before[index]).toContain(`session ${id}`);
    expect(after[index]).toContain(`session ${id}`);
  }
});

it('names the session of an approval that has ended', async () => {
  const ipc = desktop();
  ipc.world.approvals = {
    ...approvals.empty,
    head: '11',
    recent: [{ ...approvals.recent[0], id: A2, session: S2, turn: T2 }],
  };
  component = mount(App, { target: document.body });
  await flush();
  await click(nav('Approvals'));
  const row = approvalsView().querySelector('.approval-row')!;
  expect(text(row)).toContain(
    `session ${S2.slice(0, 8)} · turn ${T2.slice(0, 8)}`,
  );
  expect(row.querySelector('[title]')?.getAttribute('title')).toBe(S2);
  expect(ipc.count('managed_sessions_page')).toBe(0);
});
