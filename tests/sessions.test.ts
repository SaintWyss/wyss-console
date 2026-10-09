import { expect, it, vi } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import { parseAuthorityView } from '../src/lib/contracts/authority';
import { parseManagedSessionId } from '../src/lib/contracts/identities';
import {
  parseSessionFailure,
  parseSessionPage,
  parseSessionReceipt,
  SESSION_FAILURES,
  type SessionFailure,
  type SessionPage,
  type SessionReceipt,
} from '../src/lib/contracts/sessions';
import {
  closeSession,
  prepareSession,
  readSessionsPage,
  resumeSession,
  SessionCallError,
} from '../src/lib/desktop/sessions-client';
import {
  SessionsController,
  type SessionsClient,
  type SessionsState,
} from '../src/lib/features/sessions/controller';
import {
  failureMessage,
  readFailureMessage,
  resumeHint,
  sessionLabel,
} from '../src/lib/features/sessions/present';
import type { AuthorityObservation } from '../src/lib/shell/authority';
import views from './fixtures/authority-view-v1.json';
import pages from './fixtures/session-page-v1.json';

const loaded = parseSessionPage(pages.loaded);
const empty = parseSessionPage(pages.empty);
const [fresh, bound, closed] = loaded.sessions;
const INTENT = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
const settle = async () => {
  for (let i = 0; i < 25; i++) await Promise.resolve();
};

function connected(
  head: string | null,
  over: { restarts?: number; actual?: string } = {},
): AuthorityObservation {
  return {
    state: 'observed',
    view: parseAuthorityView({
      ...views.connected,
      authority: {
        ...views.connected.authority,
        last_sequence: head,
        restarts: over.restarts ?? 0,
      },
      agent: { ...views.connected.agent, actual: over.actual ?? 'ready' },
    }),
  };
}
const offline: AuthorityObservation = {
  state: 'observed',
  view: parseAuthorityView(views.waiting_absent),
};
const at = (page: SessionPage, head: string | null): SessionPage => ({
  ...page,
  head,
});

interface Deferred<T> {
  resolve: (value: T) => void;
  reject: (failure: SessionFailure) => void;
}
/** A client whose every call waits to be answered by the test. */
function harness() {
  const reads: (Deferred<SessionPage> & { before: string | null })[] = [];
  const intents: (Deferred<SessionReceipt> & {
    kind: string;
    args: unknown[];
  })[] = [];
  const deferred = <T>(): Deferred<T> & { promise: Promise<T> } => {
    let resolve!: (value: T) => void;
    let fail!: (error: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      fail = rej;
    });
    return {
      promise,
      resolve,
      reject: (failure) => fail(new SessionCallError(failure)),
    };
  };
  const intent =
    (kind: string) =>
    (...args: unknown[]) => {
      const d = deferred<SessionReceipt>();
      intents.push({ ...d, kind, args });
      return d.promise;
    };
  const client: SessionsClient = {
    page(before) {
      const d = deferred<SessionPage>();
      reads.push({ ...d, before });
      return d.promise;
    },
    prepare: intent('prepare'),
    resume: intent('resume'),
    close: intent('close'),
  };
  let state!: SessionsState;
  let minted = 0;
  const controller = new SessionsController(
    (next) => {
      state = next;
    },
    client,
    () => `intent-${++minted}`,
  );
  state = controller.state();
  return { controller, reads, intents, state: () => state };
}
/** Shown and connected at `head`, with its first read answered by `page`. */
async function showing(page: SessionPage, head: string | null = page.head) {
  const h = harness();
  h.controller.setVisible(true);
  h.controller.observe(connected(head));
  h.reads[0].resolve(page);
  await settle();
  return h;
}

it('accepts exactly the pages the desktop serializes', () => {
  expect(loaded).toEqual(pages.loaded);
  expect(empty).toEqual(pages.empty);
  // The head is a lower bound: a session committed during the read may sit past it.
  expect(parseSessionPage({ ...pages.loaded, head: '3' }).head).toBe('3');
  const older = { ...pages.loaded, older: '1' };
  expect(parseSessionPage(older).older).toBe('1');
});

it('rejects anything that is not a v1 page', () => {
  const page = pages.loaded;
  const [first, second, third] = page.sessions;
  const session = (over: Record<string, unknown>) => ({
    ...page,
    sessions: [{ ...second, ...over }],
  });
  const rejected: unknown[] = [
    null,
    [],
    'page',
    { ...page, schema_version: 2 },
    { ...page, stream_id: 'x' },
    { ...page, head: 9 },
    { ...page, head: '09' },
    { ...page, truncated: 'no' },
    { ...page, sessions: null },
    // More than one page of the desktop's size is not a page.
    { ...page, sessions: Array.from({ length: 26 }, () => first) },
    // Not newest first, or the same session twice.
    { ...page, sessions: [third, second, first] },
    { ...page, sessions: [first, first] },
    { ...page, sessions: [first, { ...second, id: first.id.toUpperCase() }] },
    // The older cursor is where the page ends, or nothing.
    { ...page, older: '4' },
    { ...page, older: 1 },
    { ...pages.empty, older: '1' },
    session({ id: 'not-a-uuid' }),
    session({ adapter: 'Codex CLI' }),
    session({ state: 'open' }),
    session({ revision: 2 }),
    session({ revision: '1' }),
    session({ state: 'closed', revision: 2, resumable: false }),
    session({ created_at: 'yesterday' }),
    session({ updated_at: '2026-10-08T11:00:00+02:00' }),
    session({ position: '0' }),
    session({ position: 4 }),
    session({ binding: 'attached' }),
    // A workspace exists exactly when a binding does, and is a plain absolute path.
    session({ workspace: null }),
    session({ binding: 'unbound', resumable: false }),
    session({ workspace: 'relative/path' }),
    session({ workspace: '/a\u001b[31m' }),
    session({ workspace: `/${'x'.repeat(256)}` }),
    session({ resumable: 'yes' }),
    session({ closable: 1 }),
    // No hint offers what the shown state rules out.
    session({ binding: 'unbound', workspace: null }),
    session({ runtime_session_id: '0199a213-81c0-7800-8aa1-bbab2a035a53' }),
    session({ lease_id: first.id }),
    { ...page, sessions: [{ ...third, closable: true }] },
    { ...page, sessions: [{ ...third, resumable: true }] },
    { ...page, sessions: [{ ...third, binding: 'bound_attached' }] },
  ];
  for (const value of rejected)
    expect(() => parseSessionPage(value), JSON.stringify(value)).toThrow(
      /^session_page: invalid_/,
    );
});

it('reads receipts and failures strictly', () => {
  expect(parseSessionReceipt({ id: fresh.id })).toEqual({ id: fresh.id });
  for (const value of [
    null,
    {},
    { id: 'x' },
    { id: fresh.id, state: 'prepared' },
  ])
    expect(() => parseSessionReceipt(value)).toThrow();
  for (const code of SESSION_FAILURES)
    expect(parseSessionFailure({ code, retry: true })).toEqual({
      code,
      retry: true,
    });
  // Whatever is not a named failure is an internal one, and never retried blindly.
  for (const value of [
    null,
    'gate_denied',
    { code: 'gate_denied', retry: false },
    { code: 'busy' },
    { code: 'busy', retry: 'yes' },
    { code: 'busy', retry: true, cause: 'runtime_operation_in_progress' },
    new Error('private detail'),
  ])
    expect(parseSessionFailure(value)).toEqual({
      code: 'internal',
      retry: false,
    });
});

it('sends four narrow requests and nothing that names a control method', async () => {
  const calls: [string, unknown][] = [];
  mockIPC((command, payload) => {
    calls.push([command, payload]);
    return command === 'managed_sessions_page'
      ? pages.loaded
      : { id: bound.id };
  });
  expect(await readSessionsPage(null)).toEqual(loaded);
  await readSessionsPage('4');
  expect(await prepareSession(INTENT)).toEqual({ id: bound.id });
  await resumeSession(INTENT, bound.id, 1);
  await closeSession(INTENT, bound.id, 1);
  const intent = {
    intent_id: INTENT,
    session_id: bound.id,
    expected_revision: 1,
  };
  expect(calls).toEqual([
    ['managed_sessions_page', { request: { before: null } }],
    ['managed_sessions_page', { request: { before: '4' } }],
    ['managed_session_prepare', { request: { intent_id: INTENT } }],
    ['managed_session_resume', { request: intent }],
    ['managed_session_close', { request: intent }],
  ]);
});

it('turns every bridge failure into a product failure', async () => {
  const failure = async (answer: () => unknown) => {
    mockIPC(answer);
    const error = await prepareSession(INTENT).then(
      () => undefined,
      (e: unknown) => e,
    );
    expect(error).toBeInstanceOf(SessionCallError);
    return (error as SessionCallError).failure;
  };
  expect(
    await failure(() => Promise.reject({ code: 'busy', retry: true })),
  ).toEqual({ code: 'busy', retry: true });
  // A rejection that is not the desktop's failure shape, or an answer that is not the
  // projection, is a failed bridge and not data.
  for (const answer of [
    () => Promise.reject('gate_denied'),
    () => Promise.reject({ code: 'gate_denied', diagnostic: '/private' }),
    () => ({ id: 'x' }),
    () => ({ id: bound.id, lease_id: fresh.id }),
    () => null,
  ])
    expect(await failure(answer)).toEqual({ code: 'internal', retry: false });
  mockIPC(() => ({ ...pages.loaded, sessions: 'none' }));
  await expect(readSessionsPage(null)).rejects.toMatchObject({
    failure: { code: 'internal', retry: false },
  });
});

it('reads nothing while hidden or offline and shows nothing it did not read', async () => {
  const h = harness();
  expect(h.state()).toMatchObject({ link: 'connecting', page: null });
  // Connected but not shown: no read.
  h.controller.observe(connected('9'));
  expect(h.state().link).toBe('connected');
  expect(h.reads).toHaveLength(0);
  h.controller.setVisible(true);
  expect(h.reads).toHaveLength(1);
  expect(h.reads[0].before).toBeNull();
  expect(h.state()).toMatchObject({ page: null, reading: true });
  h.reads[0].resolve(loaded);
  await settle();
  expect(h.state()).toMatchObject({ page: loaded, reading: false });

  // The connection ends: what was read on it is gone, and so is every open prompt.
  h.controller.select(bound.id);
  h.controller.confirmClose(bound.id);
  h.controller.compose(true);
  expect(h.state()).toMatchObject({ confirmingClose: bound.id });
  h.controller.observe(offline);
  expect(h.state()).toMatchObject({
    link: 'offline',
    page: null,
    composing: false,
    confirmingClose: null,
    agentReady: false,
  });
  // Offline, nothing can be started and nothing is read.
  h.controller.compose(true);
  h.controller.prepare();
  h.controller.resume(bound.id);
  h.controller.close(bound.id);
  h.controller.refresh();
  h.controller.older();
  expect(h.state().composing).toBe(false);
  expect([h.reads.length, h.intents.length]).toEqual([1, 0]);
  for (const observation of [
    { state: 'unavailable' },
    { state: 'observed', view: parseAuthorityView(views.stopped) },
  ] as AuthorityObservation[]) {
    h.controller.observe(observation);
    expect(h.state().link).toBe('offline');
  }
  h.controller.observe({ state: 'pending' });
  expect(h.state().link).toBe('connecting');
  // Back: read again from the newest page, with the selection kept.
  h.controller.observe(connected('9'));
  expect(h.reads).toHaveLength(2);
  h.reads[1].resolve(loaded);
  await settle();
  expect(h.state()).toMatchObject({ page: loaded, selected: bound.id });
});

it('re-reads once for each head the authority reports and never in a loop', async () => {
  const h = await showing(loaded);
  // The same head again, or any view change that is not a new head: nothing to read.
  h.controller.observe(connected('9'));
  h.controller.observe(connected('9', { actual: 'stopped' }));
  expect(h.state().agentReady).toBe(false);
  expect(h.reads).toHaveLength(1);

  // A new durable event wakes one read.
  h.controller.observe(connected('10'));
  expect(h.reads).toHaveLength(2);
  expect(h.state()).toMatchObject({ page: loaded, reading: true });
  // More events while it is in flight are one more read, not one each.
  h.controller.observe(connected('11'));
  h.controller.observe(connected('12'));
  expect(h.reads).toHaveLength(2);
  h.reads[1].resolve(at(loaded, '10'));
  await settle();
  expect(h.reads).toHaveLength(3);
  h.reads[2].resolve(at(loaded, '12'));
  await settle();
  expect(h.reads).toHaveLength(3);
  expect(h.state().page?.head).toBe('12');

  // A peer whose pages never catch up is read once for that head, then left alone.
  h.controller.observe(connected('20'));
  h.reads[3].resolve(at(loaded, '2'));
  await settle();
  expect(h.reads).toHaveLength(4);
  h.controller.observe(connected('20'));
  expect(h.reads).toHaveLength(4);

  // Another authority instance: what was attached there is not attached here.
  h.controller.observe(connected('20', { restarts: 1 }));
  expect(h.reads).toHaveLength(5);
  h.reads[4].resolve(at(loaded, '20'));
  await settle();
  // Hidden: events are not followed. Shown again: one read catches up.
  h.controller.setVisible(false);
  h.controller.observe(connected('21', { restarts: 1 }));
  h.controller.observe(connected('22', { restarts: 1 }));
  expect(h.reads).toHaveLength(5);
  h.controller.setVisible(true);
  expect(h.reads).toHaveLength(6);
  h.controller.setVisible(true);
  expect(h.reads).toHaveLength(6);
});

it('shows a failed read as a failure and reads again only when asked or woken', async () => {
  const h = await showing(loaded);
  h.controller.observe(connected('10'));
  h.reads[1].reject({ code: 'unavailable', retry: true });
  await settle();
  // Nothing unverified stays on screen.
  expect(h.state()).toMatchObject({
    page: null,
    reading: false,
    readFailure: { code: 'unavailable', retry: true },
  });
  // No retry storm: the same head asks for nothing.
  h.controller.observe(connected('10'));
  expect(h.reads).toHaveLength(2);
  h.controller.refresh();
  expect(h.reads).toHaveLength(3);
  expect(h.state().readFailure).toBeNull();
  h.reads[2].reject({ code: 'internal', retry: false });
  await settle();
  // A new head is a reason to try again.
  h.controller.observe(connected('11'));
  expect(h.reads).toHaveLength(4);
  h.reads[3].resolve(at(loaded, '11'));
  await settle();
  expect(h.state()).toMatchObject({ readFailure: null, reading: false });
  expect(h.state().page?.head).toBe('11');

  // An answer to a read that was given up is not shown.
  h.controller.observe(connected('12'));
  h.controller.observe(offline);
  h.reads[4].resolve(at(loaded, '12'));
  await settle();
  expect(h.state()).toMatchObject({ link: 'offline', page: null });
});

it('names one action with one intent however many times it is asked', async () => {
  const h = await showing(empty, null);
  h.controller.compose(true);
  expect(h.state().composing).toBe(true);
  h.controller.prepare();
  expect(h.state()).toMatchObject({
    composing: false,
    operation: { kind: 'prepare', session: null, status: 'pending' },
  });
  // Clicking again while it is pending sends nothing, of any kind.
  h.controller.prepare();
  h.controller.prepare();
  h.controller.compose(true);
  h.controller.retry();
  h.controller.dismiss();
  expect(h.state().composing).toBe(false);
  expect(h.intents.map((i) => [i.kind, i.args])).toEqual([
    ['prepare', ['intent-1']],
  ]);

  // The answer was lost: the outcome is unknown and the same intent settles it.
  h.intents[0].reject({ code: 'unavailable', retry: true });
  await settle();
  expect(h.state().operation).toEqual({
    kind: 'prepare',
    session: null,
    status: 'failed',
    failure: { code: 'unavailable', retry: true },
  });
  // The failure made it read again: whether the session exists is the authority's to say.
  expect(h.reads).toHaveLength(2);
  h.reads[1].resolve(empty);
  await settle();
  h.controller.retry();
  h.controller.retry();
  expect(h.intents.map((i) => i.args)).toEqual([['intent-1'], ['intent-1']]);
  expect(h.state().operation?.status).toBe('pending');
  const before = h.reads.length;
  h.intents[1].resolve({ id: fresh.id });
  await settle();
  // Accepted: nothing is assumed about the session. It is selected and read.
  expect(h.state()).toMatchObject({
    operation: null,
    selected: fresh.id,
    page: empty,
  });
  expect(h.reads).toHaveLength(before + 1);
  h.reads.at(-1)!.resolve(loaded);
  await settle();
  expect(h.state().page).toEqual(loaded);

  // A refusal is not retried under the same identity; the next action is a new one.
  h.controller.prepare();
  expect(h.intents.at(-1)!.args).toEqual(['intent-2']);
  h.intents.at(-1)!.reject({ code: 'conflict', retry: false });
  await settle();
  expect(h.state().operation?.failure).toEqual({
    code: 'conflict',
    retry: false,
  });
  h.controller.retry();
  expect(h.intents).toHaveLength(3);
  h.controller.dismiss();
  expect(h.state().operation).toBeNull();
  h.controller.retry();
  expect(h.intents).toHaveLength(3);
  h.controller.prepare();
  expect(h.intents.at(-1)!.args).toEqual(['intent-3']);
});

it('sends resume and close for the session and revision that were shown', async () => {
  const h = await showing(loaded);
  // A session that is not on the page cannot be acted on.
  const unknown = parseManagedSessionId('99999999-9999-4999-8999-999999999999');
  h.controller.resume(unknown);
  h.controller.close(unknown);
  h.controller.confirmClose(unknown);
  // Closing is asked about first, and only for a session that can be closed.
  h.controller.confirmClose(closed.id);
  expect(h.state().confirmingClose).toBeNull();
  h.controller.confirmClose(bound.id);
  expect(h.state().confirmingClose).toBe(bound.id);
  h.controller.select(fresh.id);
  expect(h.state()).toMatchObject({
    selected: fresh.id,
    confirmingClose: null,
  });
  expect(h.intents).toHaveLength(0);

  h.controller.resume(bound.id);
  expect(h.intents[0]).toMatchObject({
    kind: 'resume',
    args: ['intent-1', bound.id, 1],
  });
  expect(h.state().operation).toMatchObject({
    kind: 'resume',
    session: bound.id,
  });
  // One action at a time, whatever it is.
  h.controller.close(fresh.id);
  h.controller.confirmClose(fresh.id);
  expect(h.state().confirmingClose).toBeNull();
  expect(h.intents).toHaveLength(1);
  h.intents[0].reject({ code: 'runtime_unavailable', retry: false });
  await settle();
  // A refusal reads the session again: it may not be what was shown.
  expect(h.reads).toHaveLength(2);
  h.reads[1].resolve(loaded);
  await settle();
  h.controller.dismiss();

  h.controller.confirmClose(bound.id);
  h.controller.close(bound.id);
  expect(h.intents[1]).toMatchObject({
    kind: 'close',
    args: ['intent-2', bound.id, 1],
  });
  expect(h.state().confirmingClose).toBeNull();
  h.intents[1].resolve({ id: bound.id });
  await settle();
  // Still shown as it was read: the close is believed when the next read says so.
  expect(h.state().page?.sessions[1].state).toBe('prepared');
  expect(h.reads).toHaveLength(3);
  const after: SessionPage = {
    ...loaded,
    head: '10',
    sessions: [
      fresh,
      {
        ...bound,
        state: 'closed',
        revision: 2,
        resumable: false,
        closable: false,
      },
      closed,
    ],
  };
  h.reads[2].resolve(after);
  await settle();
  expect(h.state().page?.sessions[1].state).toBe('closed');
  // Lost while an action is pending: its answer still settles it.
  h.controller.resume(fresh.id);
  h.controller.observe(offline);
  h.intents[2].reject({ code: 'unavailable', retry: true });
  await settle();
  expect(h.state()).toMatchObject({
    link: 'offline',
    operation: { status: 'failed' },
  });
  // No retry reaches an authority that is not there.
  h.controller.retry();
  expect(h.intents).toHaveLength(3);
});

it('walks older pages one read at a time and returns to the newest', async () => {
  const newest = { ...loaded, older: '1' };
  const h = await showing(newest);
  expect(h.state().depth).toBe(0);
  h.controller.newer();
  h.controller.newest();
  expect(h.reads).toHaveLength(1);
  h.controller.older();
  expect(h.reads[1].before).toBe('1');
  expect(h.state()).toMatchObject({ depth: 1, page: null, reading: true });
  // A turn waits for the read in flight.
  h.controller.older();
  h.controller.newer();
  expect(h.reads).toHaveLength(2);
  h.reads[1].resolve(at(empty, '9'));
  await settle();
  expect(h.state()).toMatchObject({ depth: 1, page: at(empty, '9') });
  // No older page: nothing to walk into.
  h.controller.older();
  expect(h.reads).toHaveLength(2);
  // A live event re-reads the page being shown, not the newest one.
  h.controller.observe(connected('10'));
  expect(h.reads[2].before).toBe('1');
  h.reads[2].resolve(at(empty, '10'));
  await settle();
  h.controller.newer();
  expect(h.reads[3].before).toBeNull();
  h.reads[3].resolve(at(newest, '10'));
  await settle();
  expect(h.state()).toMatchObject({ depth: 0 });

  // A new session is on the newest page: creating one from an older page goes there.
  h.controller.older();
  h.reads[4].resolve(at(empty, '10'));
  await settle();
  h.controller.prepare();
  h.intents[0].resolve({ id: fresh.id });
  await settle();
  expect(h.state()).toMatchObject({ depth: 0, selected: fresh.id });
  expect(h.reads.at(-1)!.before).toBeNull();
});

it('stops answering once disposed', async () => {
  const h = await showing(loaded);
  const update = vi.fn();
  const controller = new SessionsController(update, {
    page: () => Promise.resolve(loaded),
    prepare: () => Promise.resolve({ id: fresh.id }),
    resume: () => Promise.resolve({ id: fresh.id }),
    close: () => Promise.resolve({ id: fresh.id }),
  });
  controller.setVisible(true);
  controller.observe(connected('9'));
  controller.dispose();
  await settle();
  const calls = update.mock.calls.length;
  controller.observe(connected('10'));
  controller.setVisible(false);
  controller.prepare();
  await settle();
  expect(update.mock.calls).toHaveLength(calls);
  h.controller.dispose();
  h.controller.observe(connected('30'));
  expect(h.reads).toHaveLength(1);
});

it('words sessions, hints and failures without protocol text', () => {
  expect(sessionLabel(bound)).toBe('rag-corp');
  expect(sessionLabel(fresh)).toBe('Session 33333333');
  expect(sessionLabel({ ...bound, workspace: '/' })).toBe('Session 22222222');
  expect(resumeHint(bound, true)).toBeNull();
  expect(resumeHint(bound, false)).toContain('not ready');
  expect(resumeHint(fresh, true)).toContain('No workspace');
  expect(resumeHint(closed, true)).toContain('closed');
  expect(
    resumeHint({ ...bound, binding: 'bound_attached', resumable: false }, true),
  ).toContain('Already attached');
  for (const code of SESSION_FAILURES) {
    for (const kind of ['prepare', 'resume', 'close'] as const) {
      const message = failureMessage(kind, code);
      expect(message.length).toBeGreaterThan(20);
      // Product wording: no error code, method or field name of the control protocol.
      expect(message).not.toMatch(/_|managedSession|executionLease|\{|\}/);
    }
    expect(readFailureMessage(code)).not.toMatch(/_|\{|\}/);
  }
  // An unknown outcome is never worded as a refusal.
  expect(failureMessage('close', 'unavailable')).not.toContain(
    'was not closed',
  );
  expect(failureMessage('close', 'changed')).toContain('was not closed');
});
