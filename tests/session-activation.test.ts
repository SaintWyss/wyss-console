import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  ACTIVATION_RECHECK_MS,
  ACTIVATION_WAIT_MS,
} from '../src/lib/features/sessions/controller';
import type { SessionPage } from '../src/lib/contracts/sessions';
import {
  at,
  bound,
  closed,
  composed,
  connected,
  empty,
  fresh,
  harness,
  loaded,
  offline,
  settle,
  showing,
} from './sessions-harness';

const FOLDER = '/home/dev/project';
const OTHER = '/home/dev/other';
/** The page after `fresh` was bound to the folder, detached or attached. */
const boundPage = (
  binding: 'bound_detached' | 'bound_attached',
): SessionPage => ({
  ...loaded,
  head: '12',
  sessions: [
    {
      ...fresh,
      binding,
      workspace: FOLDER,
      resumable: binding === 'bound_detached',
      activatable: false,
    },
    bound,
    closed,
  ],
});
const uncertain: SessionPage = {
  ...loaded,
  head: '12',
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
const steps = (h: ReturnType<typeof harness>) => h.steps.map((s) => s.args);

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

it('chooses a folder through the native dialog and nothing else', async () => {
  const h = await showing(empty, null);
  // No composer, no dialog.
  h.controller.chooseWorkspace();
  expect(h.dialogs).toHaveLength(0);
  h.controller.compose(true);
  expect(h.state()).toMatchObject({
    composing: true,
    composeFor: null,
    workspace: null,
    choosing: false,
  });
  // Without a folder there is nothing to activate.
  h.controller.activate();
  expect([h.intents.length, h.steps.length]).toEqual([0, 0]);

  h.controller.chooseWorkspace();
  expect(h.state().choosing).toBe(true);
  // One dialog at a time, and nothing is sent while it is open.
  h.controller.chooseWorkspace();
  h.controller.activate();
  h.controller.compose(true);
  expect(h.dialogs).toHaveLength(1);
  // Dismissed: nothing was chosen.
  h.dialogs[0].resolve(null);
  await settle();
  expect(h.state()).toMatchObject({ choosing: false, workspace: null });

  h.controller.chooseWorkspace();
  h.dialogs[1].resolve(FOLDER);
  await settle();
  expect(h.state()).toMatchObject({ workspace: FOLDER, choosing: false });
  // Changing it replaces it; dismissing the dialog keeps what was chosen.
  h.controller.chooseWorkspace();
  h.dialogs[2].resolve(OTHER);
  await settle();
  expect(h.state().workspace).toBe(OTHER);
  h.controller.chooseWorkspace();
  h.dialogs[3].resolve(null);
  await settle();
  expect(h.state().workspace).toBe(OTHER);

  // A dialog that could not be opened, or a folder that cannot be carried, is said.
  h.controller.chooseWorkspace();
  h.dialogs[4].reject({ code: 'workspace_invalid', retry: false });
  await settle();
  expect(h.state()).toMatchObject({
    choosing: false,
    workspace: OTHER,
    chooseFailure: { code: 'workspace_invalid', retry: false },
  });
  h.controller.chooseWorkspace();
  expect(h.state().chooseFailure).toBeNull();
  h.dialogs[5].resolve(FOLDER);
  await settle();

  // Opening the composer again starts without a folder: one is chosen each time.
  h.controller.compose(false);
  h.controller.compose(true);
  expect(h.state()).toMatchObject({ composing: true, workspace: null });
  // An answer for a composer that is gone chooses nothing.
  h.controller.chooseWorkspace();
  h.controller.observe(offline);
  h.dialogs[6].resolve(FOLDER);
  await settle();
  expect(h.state()).toMatchObject({
    composing: false,
    workspace: null,
    choosing: false,
  });
});

it('creates and activates a session as one action with one identity', async () => {
  const h = await showing(empty, null);
  h.controller.observe(connected(null, { actual: 'stopped' }));
  await composed(h, FOLDER);
  h.controller.activate();
  // First the session itself: prepared by the authority, under the action's identity.
  expect(h.state()).toMatchObject({
    composing: false,
    operation: {
      kind: 'activate',
      mode: 'inference',
      session: null,
      status: 'pending',
      step: 'preparing',
    },
  });
  expect(h.intents.map((i) => [i.kind, i.args])).toEqual([
    ['prepare', ['intent-1']],
  ]);
  expect(h.steps).toHaveLength(0);
  // A second click, on anything, sends nothing.
  h.controller.activate();
  h.controller.compose(true);
  h.controller.resume(bound.id);
  expect(h.state().composing).toBe(false);

  h.intents[0].resolve({ id: fresh.id });
  await settle();
  // The session exists: it is selected and read, and its activation is asked for.
  expect(h.state()).toMatchObject({
    selected: fresh.id,
    operation: { session: fresh.id, step: 'starting' },
  });
  expect(steps(h)).toEqual([['intent-1', fresh.id, 1, FOLDER, 'inference']]);
  expect(h.reads).toHaveLength(2);
  h.reads[1].resolve(loaded);
  await settle();

  // The authority was asked to start Codex: the step waits. Nothing is sent meanwhile.
  h.steps[0].resolve({ stage: 'runtime_starting' });
  await settle();
  expect(h.state().operation).toMatchObject({ step: 'starting' });
  expect(h.steps).toHaveLength(1);
  // The authority reports the runtime ready: the same request is sent again.
  h.controller.observe(connected('10'));
  h.reads.at(-1)!.resolve(at(loaded, '10'));
  await settle();
  expect(steps(h)).toEqual([
    ['intent-1', fresh.id, 1, FOLDER, 'inference'],
    ['intent-1', fresh.id, 1, FOLDER, 'inference'],
  ]);
  expect(h.state().operation).toMatchObject({ step: 'binding' });

  // Bound: that much is true now, so it is read, and the next step attaches.
  const reads = h.reads.length;
  h.steps[1].resolve({ stage: 'bound' });
  await settle();
  expect(h.state().operation).toMatchObject({ step: 'attaching' });
  expect(h.steps).toHaveLength(3);
  expect(h.steps[2].args).toEqual([
    'intent-1',
    fresh.id,
    1,
    FOLDER,
    'inference',
  ]);
  expect(h.reads.length).toBeGreaterThan(reads);
  h.reads.at(-1)!.resolve(boundPage('bound_detached'));
  await settle();
  // Nothing is shown as ready before the authority says so.
  expect(h.state().page?.sessions[0].binding).toBe('bound_detached');
  expect(h.state().operation?.status).toBe('pending');

  h.steps[2].resolve({ stage: 'ready' });
  await settle();
  expect(h.state().operation).toBeNull();
  // Ready is what the next read shows, not what the answer implied.
  h.reads.at(-1)!.resolve(boundPage('bound_attached'));
  await settle();
  expect(h.state().page?.sessions[0]).toMatchObject({
    binding: 'bound_attached',
    workspace: FOLDER,
  });
  // One session was asked for, once; every step named the same action.
  expect(h.intents).toHaveLength(1);
  expect(new Set(h.steps.map((s) => s.args[0]))).toEqual(new Set(['intent-1']));
});

it('activates an existing session that has no workspace, and only such a session', async () => {
  const h = await showing(loaded);
  // Bound, closed or unknown sessions have nothing to activate.
  for (const session of [bound.id, closed.id]) {
    h.controller.compose(true, session);
    expect(h.state().composing).toBe(false);
  }
  await composed(h, FOLDER, fresh.id);
  expect(h.state()).toMatchObject({ composeFor: fresh.id, workspace: FOLDER });
  h.controller.activate();
  // No new session is prepared: the one that was shown is activated at its revision.
  expect(h.intents).toHaveLength(0);
  expect(steps(h)).toEqual([['intent-1', fresh.id, 1, FOLDER, 'inference']]);
  // The runtime is ready, as the authority view says: the first step is the binding.
  expect(h.state().operation).toEqual({
    kind: 'activate',
    mode: 'inference',
    session: fresh.id,
    status: 'pending',
    step: 'binding',
  });
  h.steps[0].resolve({ stage: 'bound' });
  await settle();
  h.steps[1].resolve({ stage: 'ready' });
  await settle();
  expect(h.state().operation).toBeNull();
  expect(h.steps).toHaveLength(2);

  // The composer of a session that stops being one to activate closes by itself.
  h.reads.at(-1)!.resolve(loaded);
  await settle();
  await composed(h, FOLDER, fresh.id);
  h.controller.observe(connected('12'));
  h.reads.at(-1)!.resolve(boundPage('bound_attached'));
  await settle();
  expect(h.state().composing).toBe(false);
  h.controller.activate();
  expect(h.steps).toHaveLength(2);

  // While the page it was shown on is being read again, the session cannot be acted
  // on: nothing is sent for a session that is not on a page that was read.
  const paged = await showing({ ...loaded, older: '1' });
  await composed(paged, FOLDER, fresh.id);
  paged.controller.older();
  expect(paged.state()).toMatchObject({ page: null, composing: true });
  paged.controller.activate();
  expect([paged.steps.length, paged.intents.length]).toEqual([0, 0]);
});

it('waits for the authority to report a change and never past its limit', async () => {
  const h = await showing(loaded);
  h.controller.observe(connected('9', { actual: 'stopped' }));
  await composed(h, FOLDER, fresh.id);
  h.controller.activate();
  expect(h.state().operation).toMatchObject({ step: 'starting' });
  h.steps[0].resolve({ stage: 'runtime_starting' });
  await settle();

  // Time alone, short of the recheck, asks for nothing.
  await vi.advanceTimersByTimeAsync(ACTIVATION_RECHECK_MS - 1);
  expect(h.steps).toHaveLength(1);
  // With no change reported it asks again, sparsely.
  await vi.advanceTimersByTimeAsync(1);
  expect(h.steps).toHaveLength(2);
  h.steps[1].resolve({ stage: 'runtime_starting' });
  await settle();
  // Any change of the authority view ends the wait at once: one request, not one each.
  h.controller.observe(connected('9', { actual: 'initializing' }));
  await settle();
  expect(h.steps).toHaveLength(3);
  h.controller.observe(connected('9', { actual: 'ready' }));
  h.controller.observe(connected('10'));
  await settle();
  expect(h.steps).toHaveLength(3);
  // The authority is still binding: the same wait, under the binding's words.
  h.steps[2].resolve({ stage: 'creating' });
  await settle();
  expect(h.state().operation).toMatchObject({ step: 'binding' });

  // A runtime that never comes up: the action ends at the limit, unknown, not refused.
  let asked = h.steps.length;
  for (
    let elapsed = 0;
    elapsed < ACTIVATION_WAIT_MS + ACTIVATION_RECHECK_MS;
    elapsed += ACTIVATION_RECHECK_MS
  ) {
    await vi.advanceTimersByTimeAsync(ACTIVATION_RECHECK_MS);
    if (h.steps.length > asked) {
      asked = h.steps.length;
      h.steps.at(-1)!.resolve({ stage: 'runtime_starting' });
      await settle();
    }
  }
  expect(h.state().operation).toEqual({
    kind: 'activate',
    mode: 'inference',
    session: fresh.id,
    status: 'failed',
    failure: { code: 'runtime_unavailable', retry: true },
  });
  // Bounded: one request each recheck, and none after the limit.
  expect(asked).toBeLessThanOrEqual(
    4 + ACTIVATION_WAIT_MS / ACTIVATION_RECHECK_MS,
  );
  await vi.advanceTimersByTimeAsync(ACTIVATION_WAIT_MS);
  expect(h.steps).toHaveLength(asked);
  // Retry is the same action: the same identity, the same session, a fresh wait.
  h.controller.retry();
  expect(h.steps.at(-1)!.args).toEqual([
    'intent-1',
    fresh.id,
    1,
    FOLDER,
    'inference',
  ]);
});

it('goes on through navigation without a second activation or a second read loop', async () => {
  const h = await showing(loaded);
  await composed(h, FOLDER, fresh.id);
  h.controller.activate();
  h.steps[0].resolve({ stage: 'runtime_starting' });
  await settle();
  // Sessions -> Console -> Sessions while it waits, and again while a step is in flight.
  const reads = h.reads.length;
  h.controller.setVisible(false);
  h.controller.setVisible(true);
  h.controller.observe(connected('10'));
  await settle();
  expect(h.steps).toHaveLength(2);
  for (let i = 0; i < 6; i++) {
    h.controller.setVisible(false);
    h.controller.setVisible(true);
  }
  // The same action, still one request in flight; hidden, nothing was read for it.
  expect(h.steps).toHaveLength(2);
  expect(h.state().operation).toMatchObject({
    kind: 'activate',
    mode: 'inference',
    status: 'pending',
  });
  h.controller.setVisible(false);
  h.steps[1].resolve({ stage: 'bound' });
  await settle();
  expect(h.steps).toHaveLength(3);
  h.steps[2].resolve({ stage: 'ready' });
  await settle();
  expect(h.state().operation).toBeNull();
  const hidden = h.reads.length;
  // Shown again: one read says what the session is now.
  h.controller.setVisible(true);
  expect(h.reads.length).toBeLessThanOrEqual(hidden + 1);
  expect(h.reads.length).toBeGreaterThan(reads);
  expect(new Set(h.steps.map((s) => s.args[0]))).toEqual(new Set(['intent-1']));
});

it('reports a refusal once and an unknown outcome as unknown, and retries as the same action', async () => {
  const h = await showing(empty, null);
  await composed(h, FOLDER);
  h.controller.activate();
  // The session's own answer was lost: unknown, and the same identity settles it.
  h.intents[0].reject({ code: 'unavailable', retry: true });
  await settle();
  expect(h.state().operation).toEqual({
    kind: 'activate',
    mode: 'inference',
    session: null,
    status: 'failed',
    failure: { code: 'unavailable', retry: true },
  });
  h.controller.retry();
  expect(h.intents.map((i) => i.args)).toEqual([['intent-1'], ['intent-1']]);
  h.intents[1].resolve({ id: fresh.id });
  await settle();
  // A step's answer was lost: the retry does not prepare a second session.
  h.steps[0].reject({ code: 'unavailable', retry: true });
  await settle();
  expect(h.state().operation).toMatchObject({
    session: fresh.id,
    status: 'failed',
    failure: { code: 'unavailable', retry: true },
  });
  h.controller.retry();
  h.controller.retry();
  expect(h.intents).toHaveLength(2);
  expect(steps(h)).toEqual([
    ['intent-1', fresh.id, 1, FOLDER, 'inference'],
    ['intent-1', fresh.id, 1, FOLDER, 'inference'],
  ]);

  // The folder was refused: said once, never retried under the same identity.
  h.steps[1].reject({ code: 'workspace_invalid', retry: false });
  await settle();
  expect(h.state().operation?.failure).toEqual({
    code: 'workspace_invalid',
    retry: false,
  });
  h.controller.retry();
  expect(h.steps).toHaveLength(2);
  h.controller.dismiss();
  expect(h.state().operation).toBeNull();

  // The session is there, prepared and without a workspace: it is activated, not recreated.
  h.reads.at(-1)!.resolve(loaded);
  await settle();
  await composed(h, OTHER, fresh.id);
  h.controller.activate();
  expect(h.intents).toHaveLength(2);
  expect(h.steps.at(-1)!.args).toEqual([
    'intent-2',
    fresh.id,
    1,
    OTHER,
    'inference',
  ]);

  // The provider's outcome is unknown: not a refusal, and nothing is sent again.
  h.steps.at(-1)!.reject({ code: 'uncertain', retry: false });
  await settle();
  expect(h.state().operation).toEqual({
    kind: 'activate',
    mode: 'inference',
    session: fresh.id,
    status: 'failed',
    failure: { code: 'uncertain', retry: false },
  });
  const sent = h.steps.length;
  h.controller.retry();
  await vi.advanceTimersByTimeAsync(ACTIVATION_WAIT_MS);
  expect(h.steps).toHaveLength(sent);
  // The session is read as the authority holds it: uncertain, with nothing to activate.
  h.reads.at(-1)!.resolve(uncertain);
  await settle();
  h.controller.dismiss();
  h.controller.compose(true, fresh.id);
  expect(h.state().composing).toBe(false);
});

it('loses the authority in the middle and continues the same action when it is back', async () => {
  const h = await showing(loaded);
  await composed(h, FOLDER, fresh.id);
  h.controller.activate();
  h.steps[0].resolve({ stage: 'runtime_starting' });
  await settle();
  // The authority goes away while the step waits: the wait ends and the next request
  // is answered by the desktop, which could not reach it.
  h.controller.observe(offline);
  await settle();
  expect(h.steps).toHaveLength(2);
  h.steps[1].reject({ code: 'unavailable', retry: true });
  await settle();
  expect(h.state()).toMatchObject({
    link: 'offline',
    page: null,
    operation: { status: 'failed', failure: { retry: true } },
  });
  // No retry reaches an authority that is not there.
  h.controller.retry();
  expect(h.steps).toHaveLength(2);
  // It returns: the same action goes on, and what the session is comes from its steps.
  h.controller.observe(connected('9', { restarts: 1 }));
  h.controller.retry();
  expect(h.steps.at(-1)!.args).toEqual([
    'intent-1',
    fresh.id,
    1,
    FOLDER,
    'inference',
  ]);
  h.steps.at(-1)!.resolve({ stage: 'ready' });
  await settle();
  expect(h.state().operation).toBeNull();
});

it('gives up on an uncertain attempt only when asked and only for such a session', async () => {
  const h = await showing(uncertain);
  // Nothing to discard on a session whose binding is not uncertain.
  for (const session of [bound.id, closed.id]) {
    h.controller.confirmDiscard(session);
    h.controller.discard(session);
  }
  expect(h.state().confirmingDiscard).toBeNull();
  expect(h.intents).toHaveLength(0);

  h.controller.confirmDiscard(fresh.id);
  expect(h.state().confirmingDiscard).toBe(fresh.id);
  // It is asked about: opening the other confirmation closes this one.
  h.controller.confirmClose(fresh.id);
  expect(h.state()).toMatchObject({
    confirmingDiscard: null,
    confirmingClose: fresh.id,
  });
  h.controller.confirmDiscard(fresh.id);
  expect(h.state().confirmingClose).toBeNull();
  h.controller.confirmDiscard(null);
  expect(h.intents).toHaveLength(0);

  h.controller.confirmDiscard(fresh.id);
  h.controller.discard(fresh.id);
  expect(h.state()).toMatchObject({
    confirmingDiscard: null,
    operation: { kind: 'abandon', session: fresh.id, status: 'pending' },
  });
  // The session and the action's identity, and nothing that names the attempt.
  expect(h.intents.map((i) => [i.kind, i.args])).toEqual([
    ['abandon', ['intent-1', fresh.id]],
  ]);
  h.controller.discard(fresh.id);
  expect(h.intents).toHaveLength(1);
  h.intents[0].resolve({ id: fresh.id });
  await settle();
  // Accepted: what the session is now is read, and then it can be activated again.
  expect(h.state().operation).toBeNull();
  expect(h.state().page?.sessions[0].binding).toBe('indeterminate');
  h.reads.at(-1)!.resolve(at(loaded, '13'));
  await settle();
  h.controller.compose(true, fresh.id);
  expect(h.state()).toMatchObject({ composing: true, composeFor: fresh.id });
  // A confirmation for a session that stopped being uncertain closes by itself.
  h.controller.compose(false);
  h.controller.observe(connected('14'));
  h.reads.at(-1)!.resolve(uncertain);
  await settle();
  h.controller.confirmDiscard(fresh.id);
  h.controller.observe(connected('15'));
  h.reads.at(-1)!.resolve(at(loaded, '15'));
  await settle();
  expect(h.state().confirmingDiscard).toBeNull();
});

it('stops a waiting activation when it is disposed', async () => {
  const h = await showing(loaded);
  await composed(h, FOLDER, fresh.id);
  h.controller.activate();
  h.steps[0].resolve({ stage: 'runtime_starting' });
  await settle();
  expect(vi.getTimerCount()).toBe(1);
  h.controller.dispose();
  // The wait ends with it: no timer is left behind to fire later.
  expect(vi.getTimerCount()).toBe(0);
  await settle();
  await vi.advanceTimersByTimeAsync(ACTIVATION_WAIT_MS * 2);
  h.controller.observe(connected('30'));
  await settle();
  // Nothing more is asked.
  expect(h.steps).toHaveLength(1);
});

/** `bound` as the authority reports it once it is attached, in `mode`. */
const attachedPage = (mode: 'inference' | 'local_tools'): SessionPage => ({
  ...loaded,
  head: '12',
  runtime:
    mode === 'local_tools'
      ? { mode, workspace: bound.workspace }
      : { mode, workspace: null },
  sessions: [fresh, { ...bound, binding: 'bound_attached', mode }, closed],
});

it('creates a session in Inference unless Local Tools is chosen, every time', async () => {
  const h = await showing(empty, null);
  await composed(h, FOLDER);
  // The composer opens in Inference: tools are never what a session gets by default.
  expect(h.state().mode).toBe('inference');
  h.controller.setMode('local_tools');
  expect(h.state().mode).toBe('local_tools');
  h.controller.activate();
  h.intents[0].resolve({ id: fresh.id });
  await settle();
  // The mode travels as a name with the folder the user selected. Nothing else does.
  expect(steps(h)).toEqual([['intent-1', fresh.id, 1, FOLDER, 'local_tools']]);
  expect(h.state().operation).toMatchObject({
    kind: 'activate',
    mode: 'local_tools',
  });
  h.steps[0].resolve({ stage: 'ready' });
  await settle();
  // Opened again, it is Inference again: the choice is made each time, not remembered.
  h.controller.compose(true);
  expect(h.state()).toMatchObject({ composing: true, mode: 'inference' });
  // It can be opened asking for Local Tools by name, and only then starts there.
  h.controller.compose(false);
  h.controller.compose(true, null, 'local_tools');
  expect(h.state().mode).toBe('local_tools');
  // The mode cannot be changed with nothing being composed.
  h.controller.compose(false);
  h.controller.setMode('inference');
  expect(h.state().mode).toBe('local_tools');
});

it('says the runtime is being stopped when another mode has to give way', async () => {
  const h = await showing(loaded);
  await composed(h, FOLDER, fresh.id);
  h.controller.setMode('local_tools');
  h.controller.activate();
  // The runtime runs in another mode: the authority was asked to stop it.
  h.steps[0].resolve({ stage: 'runtime_stopping' });
  await settle();
  expect(h.state().operation).toMatchObject({
    kind: 'activate',
    status: 'pending',
    step: 'stopping',
    mode: 'local_tools',
  });
  // Nothing is asked again until the authority reports a change.
  expect(h.steps).toHaveLength(1);
  h.controller.observe(connected('9', { actual: 'stopped' }));
  await settle();
  expect(h.steps).toHaveLength(2);
  expect(h.steps[1].args).toEqual(h.steps[0].args);
  h.steps[1].resolve({ stage: 'runtime_starting' });
  await settle();
  expect(h.state().operation).toMatchObject({ step: 'starting' });
  h.controller.observe(connected('10'));
  await settle();
  h.steps[2].resolve({ stage: 'ready' });
  await settle();
  expect(h.state().operation).toBeNull();
});

it('enters a mode with a bound session only after it was confirmed, naming no folder', async () => {
  const h = await showing(loaded);
  h.controller.select(bound.id);
  // Asked for: nothing is sent until the user confirms what it does.
  h.controller.confirmMode(bound.id, 'local_tools');
  expect(h.state().confirmingMode).toEqual({
    session: bound.id,
    mode: 'local_tools',
  });
  expect(h.steps).toHaveLength(0);
  // Backed out of: nothing was asked.
  h.controller.confirmMode(null);
  h.controller.enterMode();
  expect(h.steps).toHaveLength(0);

  h.controller.confirmMode(bound.id, 'local_tools');
  h.controller.enterMode();
  // The session, the revision that was shown and the mode. No workspace, no profile:
  // the folder is the one the authority holds for the session.
  expect(steps(h)).toEqual([['intent-1', bound.id, 1, 'local_tools']]);
  expect(h.state()).toMatchObject({
    confirmingMode: null,
    operation: { kind: 'mode', session: bound.id, mode: 'local_tools' },
  });
  // One action at a time: a second confirmation or click sends nothing.
  h.controller.confirmMode(bound.id, 'inference');
  h.controller.enterMode();
  expect(h.steps).toHaveLength(1);
  h.steps[0].resolve({ stage: 'runtime_stopping' });
  await settle();
  expect(h.state().operation).toMatchObject({ step: 'stopping' });
  h.controller.observe(connected('10', { actual: 'stopped' }));
  await settle();
  h.steps[1].resolve({ stage: 'runtime_starting' });
  await settle();
  h.controller.observe(connected('11'));
  await settle();
  // Every step is the same request: the same action for the authority.
  expect(new Set(steps(h).map((s) => JSON.stringify(s))).size).toBe(1);
  h.steps[2].resolve({ stage: 'ready' });
  await settle();
  expect(h.state().operation).toBeNull();
  // What the session is now is read, not assumed.
  h.reads.at(-1)!.resolve(attachedPage('local_tools'));
  await settle();
  expect(h.state().page?.sessions[1]).toMatchObject({
    id: bound.id,
    mode: 'local_tools',
  });
  expect(h.state().page?.runtime).toEqual({
    mode: 'local_tools',
    workspace: bound.workspace,
  });
});

it('shows a refused change of mode as the authority answered it and retries as the same action', async () => {
  const h = await showing(loaded);
  h.controller.confirmMode(bound.id, 'local_tools');
  h.controller.enterMode();
  // A turn is running: the runtime is not restarted under it.
  h.steps[0].reject({ code: 'turn_active', retry: false });
  await settle();
  expect(h.state().operation).toEqual({
    kind: 'mode',
    session: bound.id,
    status: 'failed',
    failure: { code: 'turn_active', retry: false },
    mode: 'local_tools',
  });
  // A refusal is not sent again by a retry.
  h.controller.retry();
  expect(h.steps).toHaveLength(1);
  h.controller.dismiss();

  // The provider refused the session in that mode: said as that.
  h.reads.at(-1)?.resolve(loaded);
  await settle();
  h.controller.confirmMode(bound.id, 'local_tools');
  h.controller.enterMode();
  h.steps[1].reject({ code: 'rejected', retry: false });
  await settle();
  expect(h.state().operation).toMatchObject({
    kind: 'mode',
    failure: { code: 'rejected' },
  });
  h.controller.dismiss();
  h.reads.at(-1)?.resolve(loaded);
  await settle();

  // An unknown outcome is retried with the identity the action had.
  h.controller.confirmMode(bound.id, 'inference');
  h.controller.enterMode();
  h.steps[2].reject({ code: 'unavailable', retry: true });
  await settle();
  h.controller.retry();
  expect(h.steps[3].args).toEqual(h.steps[2].args);
  expect(h.steps[3].args).toEqual(['intent-3', bound.id, 1, 'inference']);
});

it('offers a change of mode only for a session that has a workspace', async () => {
  const h = await showing(loaded);
  // No workspace, or closed: there is none to run a mode for.
  for (const session of [fresh, closed]) {
    h.controller.confirmMode(session.id, 'local_tools');
    expect(h.state().confirmingMode).toBeNull();
    h.controller.enterMode();
  }
  expect(h.steps).toHaveLength(0);
  // A confirmation is dropped when what it was for is no longer shown as eligible.
  h.controller.confirmMode(bound.id, 'local_tools');
  expect(h.state().confirmingMode).not.toBeNull();
  h.controller.observe(connected('13'));
  h.reads.at(-1)!.resolve(
    at(
      {
        ...loaded,
        sessions: [
          fresh,
          {
            ...bound,
            state: 'closed',
            revision: 2,
            binding: 'bound_detached',
            resumable: false,
            closable: false,
          },
          closed,
        ],
      },
      '13',
    ),
  );
  await settle();
  expect(h.state().confirmingMode).toBeNull();
  h.controller.enterMode();
  expect(h.steps).toHaveLength(0);
  // Losing the authority drops it too: nothing is entered on a stale confirmation.
  const again = await showing(loaded);
  again.controller.confirmMode(bound.id, 'local_tools');
  again.controller.observe(offline);
  expect(again.state().confirmingMode).toBeNull();
  again.controller.enterMode();
  expect(again.steps).toHaveLength(0);
});
