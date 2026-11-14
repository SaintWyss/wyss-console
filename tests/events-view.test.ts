import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { mockIPC } from '@tauri-apps/api/mocks';
import App from '../src/App.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import { parseTerminalId } from '../src/lib/contracts/identities';
import { listing, S2, T2 } from './agent-harness';
import {
  A1,
  OTHER_STREAM,
  rawPage,
  row,
  S1,
  STREAM,
  T1,
} from './events-harness';
import approvals from './fixtures/approval-list-v1.json';
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
const flush = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
  flushSync();
};
const never = () => new Promise(() => undefined);
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const ready = (head: string | null, revision = 3, restarts = 0) => ({
  ...views.connected,
  revision,
  authority: { ...views.connected.authority, last_sequence: head, restarts },
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
});

interface Call {
  command: string;
  request: Record<string, unknown>;
}
/** A desktop whose terminal works and whose authority holds the history the test says. */
function desktop(head: string | null = '12') {
  const calls: Call[] = [];
  const world = {
    head,
    /** What a read of the newest events answers. */
    events: (): unknown => rawPage(12, 12),
    older: (request: Record<string, unknown>): unknown => {
      void request;
      return rawPage(0, 0, { head: '12' });
    },
    approval: (): unknown => approvals.command,
    /** What a read of the approvals answers. */
    approvals: (): unknown => approvals.empty,
    authority: undefined as ((view: unknown) => void) | undefined,
  };
  mockIPC((command, payload) => {
    const request =
      (payload as { request?: Record<string, unknown> })?.request ?? {};
    calls.push({ command, request });
    if (command === 'authority_state') {
      if (request.after === null) return ready(world.head);
      return new Promise((resolve) => (world.authority = resolve));
    }
    if (command === 'managed_events') return world.events();
    if (command === 'managed_events_older') return world.older(request);
    if (command === 'managed_approval_read') return world.approval();
    if (command === 'managed_approvals') return world.approvals();
    if (command === 'managed_sessions_page') return pages.loaded;
    if (command === 'managed_turns') return listing([]);
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
    /** The authority answers its follower with another view. */
    async tell(view: unknown) {
      // The follower asks again once it has taken the answer before this one.
      for (let i = 0; i < 10 && !world.authority; i++) await flush();
      const answer = world.authority!;
      world.authority = undefined;
      answer(view);
      await flush();
    },
  };
}
const nav = (label: string) =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item'),
  ).find((b) => b.textContent?.includes(label))!;
const view = () => document.querySelector<HTMLElement>('.events-feature')!;
const rows = (root: ParentNode = view()) =>
  Array.from(root.querySelectorAll<HTMLElement>('.event-row'));
const titles = (root: ParentNode = view()) =>
  rows(root).map((r) => text(r.querySelector('.event-title')));
const details = () => view().querySelector<HTMLElement>('.event-details');
const button = (label: string, root: ParentNode = view()) =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  );
async function click(target: HTMLElement | undefined) {
  expect(target).toBeDefined();
  target!.click();
  await flush();
}
/** The application with the Events view open on what the authority holds. */
async function opened(
  events: () => unknown = () => rawPage(12, 12),
  head: string | null = '12',
) {
  const ipc = desktop(head);
  ipc.world.events = events;
  component = mount(App, { target: document.body });
  await flush();
  await click(nav('Events'));
  return ipc;
}
/** Scrolls the list of events and tells the view, as a reader would. */
async function scrollTo(top: number) {
  const list = view().querySelector<HTMLElement>('.events-main')!;
  list.scrollTop = top;
  list.dispatchEvent(new Event('scroll'));
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
  document.body.innerHTML = '';
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it('is a view of its own that reads history only while it is shown', async () => {
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  // Enabled in the sidebar, and nothing was read for it yet.
  expect(nav('Events').disabled).toBe(false);
  expect(view().hidden).toBe(true);
  expect(ipc.count('managed_events')).toBe(0);
  await click(nav('Events'));
  expect(view().hidden).toBe(false);
  // Its own heading names it: no title is drawn over it.
  expect(document.querySelector('.context-title')).toBeNull();
  expect(view().querySelector('h2')?.textContent).toBe('Events');
  expect(ipc.of('managed_events')).toEqual([
    { command: 'managed_events', request: {} },
  ]);
  expect(text(view().querySelector('.sessions-count'))).toBe(
    '12 loaded · newest first',
  );
  expect(rows()).toHaveLength(12);
  // Somewhere else, the durable head moves three times: nothing is read for Events.
  await click(nav('Console'));
  await ipc.tell(ready('15', 4));
  await ipc.tell(ready('16', 5));
  expect(ipc.count('managed_events')).toBe(1);
  // Shown again: what is there now is read, once.
  ipc.world.events = () => rawPage(16, 16);
  await click(nav('Events'));
  expect(ipc.count('managed_events')).toBe(2);
  expect(rows()).toHaveLength(16);
  // The terminal was not touched by any of it.
  expect(ipc.count('create_terminal')).toBe(1);
  expect(ipc.count('close_terminal')).toBe(0);
});

it('says what it is doing before there is history to show', async () => {
  // No authority: nothing is read, and the view says why.
  const ipc = desktop();
  component = mount(App, { target: document.body });
  await flush();
  await ipc.tell(views.waiting_absent);
  await click(nav('Events'));
  expect(text(view().querySelector('.empty-title'))).toBe(
    'The authority is unavailable',
  );
  expect(text(view())).toContain('Nothing is shown until it is connected.');
  expect(ipc.count('managed_events')).toBe(0);
  await unmount(component);
  document.body.innerHTML = '';

  // A read that fails: said, with a way to ask again.
  let fail = true;
  const failing = await opened(() =>
    fail ? Promise.reject({ code: 'unavailable', retry: true }) : rawPage(0, 0),
  );
  expect(text(view().querySelector('.empty-title'))).toBe(
    'Events could not be read: the authority did not answer.',
  );
  fail = false;
  await click(button('Try again'));
  expect(failing.count('managed_events')).toBe(2);
  // A stream with nothing recorded in it.
  expect(text(view().querySelector('.empty-title'))).toBe('No events yet.');
  expect(rows()).toHaveLength(0);
});

it('reads each record as one plain line, newest first, and never as data', async () => {
  const kinds = [
    row(1, 'session_prepared'),
    row(2, 'runtime_desired', { facts: { desired: 'running' } }),
    row(3, 'runtime_ready', { facts: { runtime_version: '0.159.0' } }),
    row(4, 'session_bound'),
    row(5, 'session_attached'),
    row(6, 'lease_granted', { facts: { scopes: ['turn_start'] } }),
    row(7, 'turn_admitted'),
    row(8, 'approval_requested', {
      facts: { approval_kind: 'command_execution' },
    }),
    row(9, 'approval_declined', {
      facts: { approval_reason: 'client_decision' },
    }),
    row(10, 'turn_interrupted', { facts: { turn_reason: 'user_cancel' } }),
    row(11, 'turn_failed', {
      disposition: 'failure',
      facts: { turn_reason: 'provider_failed' },
    }),
    row(12, 'runtime_exited', {
      disposition: 'security',
      security: true,
      facts: { exit: 'safety_unproven', expected: false },
    }),
    row(13, 'other'),
  ];
  await opened(
    () => ({ ...rawPage(13, 13), events: [...kinds].reverse() }),
    '13',
  );
  expect(titles()).toEqual([
    'Authority event',
    'Codex runtime contained',
    'Managed turn failed',
    'Managed turn interrupted',
    'Approval denied',
    'Command approval requested',
    'Managed turn admitted',
    'Execution lease granted',
    'Session attached to Codex',
    'Session bound to a workspace',
    'Codex runtime ready',
    'Codex runtime start accepted',
    'Managed session prepared',
  ]);
  // A mark only where the record itself says how it ended, in plain words.
  const marks = rows().map((r) => text(r.querySelector('.pill')));
  expect(marks.slice(0, 4)).toEqual(['', 'Isolation', 'Failed', '']);
  expect(marks.join(' ')).not.toMatch(/critical|severe|alert|danger/i);
  expect(text(rows()[2])).toContain('Turn · session 22222222');
  // Nothing on the page is a record as the authority stores it.
  const page = text(view());
  expect(page).not.toMatch(/[{}]|payload|event_type|"kind"|authority\./);
  // An event of a kind this build does not know shows where it is, and nothing else.
  await click(rows()[0]);
  expect(text(details())).toContain(
    'The authority recorded an event of a kind this version of the console does not know. Nothing else of it is shown.',
  );
  expect(button('Open session', details()!)).toBeUndefined();
  // What a record says beyond its title is the authority's own reason, in a sentence.
  await click(rows()[3]);
  expect(text(details()!.querySelector('.session-hint'))).toBe(
    'Stopped at your request.',
  );
  await click(rows()[1]);
  expect(text(details()!.querySelector('.session-hint'))).toBe(
    'The authority could not prove the isolation of the runtime, and did not let it run.',
  );
  await click(rows()[7]);
  expect(text(details()!.querySelector('.session-hint'))).toBe(
    'The authority granted a lease for this session. It allowed the session to start a turn.',
  );
  await click(rows()[5]);
  expect(text(details()!.querySelector('.session-hint'))).toBe(
    'Codex proposed a command and the authority held it for a decision. The proposal itself is not part of the record.',
  );
});

it('filters the loaded events and says that is all it filters', async () => {
  const kinds = [
    'session_prepared',
    'runtime_ready',
    'turn_running',
    'approval_requested',
    'lease_granted',
    'other',
    'runtime_exited',
  ] as const;
  const ipc = await opened(() => {
    const raw = rawPage(7, 7, { kind: (at) => kinds[at - 1] });
    // What the record says is isolation is security, whatever its category.
    raw.events[0] = row(7, 'runtime_exited', {
      disposition: 'security',
      security: true,
      facts: { exit: 'inference_isolation_violation', expected: false },
    });
    return raw;
  }, '7');
  const filters = () =>
    Array.from(
      view().querySelectorAll<HTMLButtonElement>('.events-filters .chip'),
    );
  expect(filters().map((f) => text(f))).toEqual([
    'All',
    'Sessions',
    'Runtime',
    'Turns',
    'Approvals',
    'Security',
  ]);
  expect(filters().map((f) => f.getAttribute('aria-pressed'))).toEqual([
    'true',
    'false',
    'false',
    'false',
    'false',
    'false',
  ]);
  await click(button('Turns'));
  expect(titles()).toEqual(['Managed turn running']);
  expect(text(view().querySelector('.sessions-count'))).toBe(
    '7 loaded · newest first · 1 shown',
  );
  expect(text(view())).toContain(
    'Filters apply to the events loaded here, not to all of history.',
  );
  await click(button('Security'));
  expect(titles()).toEqual([
    'Codex runtime contained',
    'Execution lease granted',
  ]);
  await click(button('Runtime'));
  expect(titles()).toEqual(['Codex runtime contained', 'Codex runtime ready']);
  await click(button('All'));
  expect(titles()).toHaveLength(7);
  // Choosing a filter reads nothing.
  expect(ipc.count('managed_events')).toBe(1);
  expect(ipc.count('managed_events_older')).toBe(0);
});

it('shows what a record is about and takes the reader to where that lives', async () => {
  const ipc = await opened(
    () => ({
      ...rawPage(3, 3),
      events: [
        row(3, 'approval_requested', {
          facts: { approval_kind: 'file_change' },
        }),
        row(2, 'turn_completed', { facts: { turn_reason: 'completed' } }),
        row(1, 'session_prepared'),
      ],
    }),
    '3',
  );
  await click(rows()[1]);
  const facts = text(details()!.querySelector('.session-facts'));
  expect(text(details()!.querySelector('h3'))).toBe('Managed turn completed');
  expect(facts).toContain(`Session ${S1}`);
  expect(facts).toContain(`Turn ${T1}`);
  expect(facts).toContain(`Position 2 in stream ${STREAM.slice(0, 8)}`);
  expect(facts).toContain('2026-10-09T10:02:00Z');
  expect(text(details())).toContain(
    'This is a record of what happened then. What it is about may have changed since',
  );
  // History changes nothing: no control anywhere in it performs, repeats or undoes.
  const controls = Array.from(view().querySelectorAll('button')).map((b) =>
    text(b),
  );
  expect(
    controls.filter((label) =>
      /approve|deny|cancel|retry|start|stop|resume|close session|revoke|\brun\b|replay|again$|undo|export|save/i.test(
        label,
      ),
    ),
  ).toEqual([]);
  expect(ipc.calls.map((c) => c.command)).not.toContain('managed_turn_start');

  // Narrow to this turn, and widen again.
  await click(button('Show events of this turn', details()!));
  expect(titles()).toEqual([
    'File change approval requested',
    'Managed turn completed',
  ]);
  expect(text(view().querySelector('.events-scope'))).toContain(
    'Turn bbbbbbbb',
  );
  await click(button('Clear'));
  expect(titles()).toHaveLength(3);

  // To the turn's session in Agents: what its turns are is read there.
  await click(button('Open in Agents', details()!));
  expect(document.querySelector('.context-title')?.textContent).toBe('Agents');
  expect(ipc.of('managed_turns').at(-1)!.request).toEqual({ session_id: S1 });
  // The tab is named for the session's folder, as read, and not left under its identity.
  expect(text(document.querySelector('.agent-feature [role="tab"]'))).toBe(
    'rag-corp',
  );
  // And to the session in Sessions, selected.
  await click(nav('Events'));
  await click(button('Open session', details()!));
  expect(document.querySelector('.context-title')).toBeNull();
  expect(view().hidden).toBe(true);
  expect(
    document.querySelector('.content > section:not([hidden]) h2')?.textContent,
  ).toBe('Managed sessions');
  expect(
    text(document.querySelector('.sessions-feature .session-details h3')),
  ).toBe('rag-corp');
  // From a session's details, its loaded history.
  await click(button('Events', document.querySelector('.sessions-feature')!));
  expect(document.querySelector('.context-title')).toBeNull();
  expect(view().hidden).toBe(false);
  expect(view().querySelector('h2')?.textContent).toBe('Events');
  // Sessions has listed it, so the scope is named for it and not only by its identity.
  expect(text(view().querySelector('.events-scope'))).toContain('rag-corp');
  expect(titles()).toHaveLength(3);
});

it('asks the authority what an approval is now, and says when it no longer knows', async () => {
  const ipc = await opened(
    () => ({
      ...rawPage(1, 1),
      events: [
        row(1, 'approval_requested', {
          facts: { approval_kind: 'command_execution' },
        }),
      ],
    }),
    '1',
  );
  await click(rows()[0]);
  expect(text(details()!.querySelector('.session-facts'))).toContain(
    `Approval ${A1}`,
  );
  // The record says it was requested. Whether it still waits is not the record's to say.
  expect(text(details())).not.toMatch(/Now:|pending|Pending/);
  expect(ipc.count('managed_approval_read')).toBe(0);
  // It ended since: the authority says so.
  ipc.world.approval = () => ({
    approval: { ...approvals.recent[0], id: A1 },
    workspace: '/home/dev/rag-corp',
    command: null,
    files: null,
    file: null,
  });
  await click(button('Check what it is now', details()!));
  expect(ipc.of('managed_approval_read')).toEqual([
    {
      command: 'managed_approval_read',
      request: { approval_id: A1, file: null, offset: null },
    },
  ]);
  expect(text(details())).toContain(
    'Now: Approved. The authority no longer holds what it was about.',
  );
  // The authority has nothing of it any more: history is still history.
  ipc.world.approval = () =>
    Promise.reject({ code: 'not_found', retry: false });
  await click(button('Check what it is now', details()!));
  expect(text(details())).toContain(
    'Approval detail is no longer available. The record above is still what happened.',
  );
  ipc.world.approval = () =>
    Promise.reject({ code: 'unavailable', retry: true });
  await click(button('Check what it is now', details()!));
  expect(text(details())).toContain(
    'The approval could not be read from the authority.',
  );
  await click(button('Open Approvals', details()!));
  expect(document.querySelector('.context-title')).toBeNull();
  expect(view().hidden).toBe(true);
  expect(
    document.querySelector('.content > section:not([hidden]) h2')?.textContent,
  ).toBe('Approvals');
});

it('loads older history when asked, and says where history ends', async () => {
  const ipc = await opened(() => rawPage(120, 50), '120');
  expect(text(view())).toContain('Older events exist and are not loaded.');
  ipc.world.older = () => rawPage(70, 50, { head: '120' });
  await click(button('Load older'));
  expect(ipc.of('managed_events_older')).toEqual([
    {
      command: 'managed_events_older',
      request: { stream_id: STREAM, before: '71' },
    },
  ]);
  expect(rows()).toHaveLength(100);
  expect(text(view().querySelector('.sessions-count'))).toBe(
    '100 loaded · newest first',
  );
  // Down to the first event: the stream begins there, and nothing more is offered.
  ipc.world.older = () => rawPage(20, 20, { head: '120' });
  await click(button('Load older'));
  expect(ipc.of('managed_events_older').at(-1)!.request).toEqual({
    stream_id: STREAM,
    before: '21',
  });
  expect(rows()).toHaveLength(120);
  expect(button('Load older')).toBeUndefined();
  expect(text(view().querySelector('.events-end'))).toBe(
    'This is the beginning of the event stream.',
  );
  // No event is on the page twice.
  const order = rows().map((r) => text(r.querySelector('.event-time')));
  expect(order).toHaveLength(120);
  await unmount(component!);
  document.body.innerHTML = '';

  // Where older history is gone, that is what is said. It is never skipped in silence.
  const gap = await opened(() => rawPage(120, 50), '120');
  gap.world.older = () => rawPage(70, 30, { head: '120', gap: true });
  await click(button('Load older'));
  expect(rows()).toHaveLength(80);
  expect(text(view().querySelector('.events-end'))).toBe(
    'History before this point is unavailable.',
  );
  expect(button('Load older')).toBeUndefined();
});

it('adds new events at the newest end and never moves a reader of older history', async () => {
  const ipc = await opened(() => rawPage(120, 50), '120');
  // At the newest end: a new event is read in when the head moves.
  ipc.world.events = () => rawPage(121, 50);
  await ipc.tell(ready('121', 4));
  expect(ipc.count('managed_events')).toBe(2);
  expect(rows()).toHaveLength(51);
  expect(view().querySelector('.events-newer')).toBeNull();
  // Reading further down: the head moves, and nothing on the page changes.
  await scrollTo(600);
  const before = rows().map((r) => text(r));
  ipc.world.events = () => rawPage(125, 50);
  await ipc.tell(ready('125', 5));
  expect(ipc.count('managed_events')).toBe(2);
  expect(rows().map((r) => text(r))).toEqual(before);
  expect(text(view().querySelector('.events-newer'))).toBe(
    'New events available',
  );
  // The reader asks for them.
  await click(button('New events available'));
  expect(ipc.count('managed_events')).toBe(3);
  expect(view().querySelector('.events-newer')).toBeNull();
  expect(rows()).toHaveLength(50);
});

it('draws a boundary where the authority holds another stream', async () => {
  const ipc = await opened(() => rawPage(120, 50), '120');
  // The same authority restarted on its own stream: history continues, and it says so.
  ipc.world.events = () => rawPage(122, 50);
  await ipc.tell(ready('122', 4, 1));
  expect(text(view())).toContain(
    'The authority restarted. It kept the same event stream',
  );
  expect(view().querySelector('.events-boundary')).toBeNull();
  expect(rows()).toHaveLength(52);
  await click(button('Dismiss'));
  expect(text(view())).not.toContain('The authority restarted');

  // Another stream on the same socket: its positions start over.
  ipc.world.events = () => rawPage(2, 2, { stream: OTHER_STREAM });
  await ipc.tell(ready('2', 5, 2));
  const boundary = view().querySelector('.events-boundary')!;
  expect(text(boundary)).toContain('New authority event stream');
  expect(text(boundary)).toContain(STREAM.slice(0, 8));
  expect(text(boundary)).toContain('positions of the two are unrelated');
  // Above it, the two events of the new stream; below it, what was shown before.
  const lists = view().querySelectorAll('.events-list');
  expect(rows(lists[0])).toHaveLength(2);
  expect(rows(lists[1])).toHaveLength(52);
  expect(
    boundary.compareDocumentPosition(lists[1]) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(text(view().querySelector('.events-end'))).toBe(
    'This is the beginning of the event stream.',
  );
  // Nothing is asked of the old stream again.
  expect(ipc.count('managed_events_older')).toBe(0);
});

it('shows a record of a replaced stream as itself, whatever now has its position', async () => {
  const ipc = await opened(() => rawPage(5, 5), '5');
  // Another stream, as long as the one before: every position exists in both.
  ipc.world.events = () =>
    rawPage(5, 5, { stream: OTHER_STREAM, kind: () => 'session_prepared' });
  await ipc.tell(ready('5', 4, 1));
  const lists = () => view().querySelectorAll('.events-list');
  expect(titles(lists()[0])[0]).toBe('Managed session prepared');
  expect(titles(lists()[1])[0]).toBe('Managed turn running');
  await click(rows(lists()[1])[0]);
  expect(text(details()!.querySelector('h3'))).toBe('Managed turn running');
  expect(text(details()!.querySelector('.session-facts'))).toContain(
    `Position 5 in stream ${STREAM.slice(0, 8)}`,
  );
  expect(
    rows().filter((r) => r.getAttribute('aria-pressed') === 'true'),
  ).toHaveLength(1);
  // What it is about lived in that stream's authority: nothing of it is offered to be
  // opened or read from the one that answers now.
  expect(text(details())).toContain(
    'This record was read from a stream the authority no longer holds here.',
  );
  expect(details()!.querySelectorAll('button.action')).toHaveLength(0);
  expect(button('Open session', details()!)).toBeUndefined();
  const reads = ipc.count('managed_approval_read');
  await click(rows(lists()[0])[0]);
  expect(text(details()!.querySelector('h3'))).toBe('Managed session prepared');
  expect(text(details()!.querySelector('.session-facts'))).toContain(
    `Position 5 in stream ${OTHER_STREAM.slice(0, 8)}`,
  );
  // A record of the stream the authority holds links to where its subject lives.
  expect(button('Open session', details()!)).toBeDefined();
  expect(text(details())).toContain(
    'that is read from the authority where it lives',
  );
  expect(ipc.count('managed_approval_read')).toBe(reads);
  // A filter is applied to each stream by itself: neither list takes a row of the other.
  await click(button('Sessions'));
  expect(rows(lists()[0])).toHaveLength(5);
  expect(view().querySelector('.events-boundary')).not.toBeNull();
  expect(rows(lists()[1])).toHaveLength(0);
});

it('keeps loaded history when the authority goes away, and says it is from before', async () => {
  const ipc = await opened(() => rawPage(120, 50), '120');
  await ipc.tell(views.waiting_absent);
  expect(text(view().querySelector('.sessions-status'))).toBe(
    'Authority offline. Showing previously loaded history: it was read before the connection was lost and nothing newer is known.',
  );
  expect(rows()).toHaveLength(50);
  expect(button('Refresh')!.disabled).toBe(true);
  expect(button('Load older')!.disabled).toBe(true);
  const read = ipc.count('managed_events');
  await click(rows()[0]);
  expect(details()).not.toBeNull();
  expect(ipc.count('managed_events')).toBe(read);
  // Back: it is read again, and what is shown is what the authority holds now.
  ipc.world.events = () => rawPage(124, 50);
  await ipc.tell(ready('124', 6));
  expect(ipc.count('managed_events')).toBe(read + 1);
  expect(view().querySelector('.sessions-status')).toBeNull();
  expect(rows()).toHaveLength(54);
  expect(button('Refresh')!.disabled).toBe(false);
});

it('identifies an approval by its session when Events was opened first and Sessions never was', async () => {
  const twin = {
    ...approvals.command,
    approval: {
      ...approvals.command.approval,
      id: 'eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee',
      session: S2,
      turn: T2,
    },
  };
  const ipc = await opened();
  expect(rows().length).toBeGreaterThan(0);
  // Two sessions in one directory have an action waiting.
  ipc.world.approvals = () => ({
    ...approvals.empty,
    head: '12',
    pending: [approvals.command, twin],
  });
  await click(nav('Approvals'));
  const cards = Array.from(
    document.querySelectorAll<HTMLElement>('.approvals-feature .approval-card'),
  );
  expect(cards).toHaveLength(2);
  // The Sessions page was not needed to say whose they are.
  expect(ipc.count('managed_sessions_page')).toBe(0);
  expect(
    cards.map((card) => text(card.querySelector('.approval-context'))),
  ).toEqual([
    `rag-corp · session ${approvals.command.approval.session} · turn ${approvals.command.approval.turn.slice(0, 8)}`,
    `rag-corp · session ${S2} · turn ${T2.slice(0, 8)}`,
  ]);
});
