import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Raw } from './f8-harness';
import {
  AUTHORITY,
  OTHER_AUTHORITY,
  SESSION,
  copy,
  detailWith,
  failure,
  never,
  real,
  rig,
  runtimeAs,
  settle,
} from './f8-harness';
import { parseTurnReading, type WatchInfo } from '../src/lib/contracts/f8';
import {
  CALL_LIMIT_MS,
  CONFIRM_LIMIT_MS,
} from '../src/lib/features/f8/controller';
import { parseF8ApprovalDetail as parseApprovalDetail } from '../src/lib/contracts/f8';

type Rig = ReturnType<typeof rig>;
let current: Rig | undefined;
const make = () => (current = rig());
afterEach(() => {
  current?.controller.dispose();
  current = undefined;
});

describe('whether the interface exists', () => {
  it('is inactive when the desktop says so, and asks for nothing else', async () => {
    const r = make();
    r.client.state.mockResolvedValueOnce(real.inactive);
    await r.controller.start();
    await r.controller.refresh();
    r.controller.setVisible(true);
    await settle();
    expect(r.state.gate).toBe('inactive');
    expect(r.client.sessions).not.toHaveBeenCalled();
    expect(r.client.wait).not.toHaveBeenCalled();
  });

  it('is inactive when the answer is not the projection, never a half-active one', async () => {
    const r = make();
    r.client.state.mockRejectedValueOnce(failure('internal'));
    await r.controller.start();
    expect(r.state.gate).toBe('inactive');
    expect(r.state.sessions).toBeNull();
  });

  it('is active, with the authority it greeted, and shows nothing until it is visible', async () => {
    const r = make();
    await r.controller.start();
    expect(r.state.gate).toBe('active');
    expect(r.state.authority?.instance_id).toBe(AUTHORITY);
    expect(r.client.sessions).not.toHaveBeenCalled();
    r.controller.setVisible(true);
    await settle();
    expect(r.client.sessions).toHaveBeenCalledWith(null);
    expect(r.state.sessions?.sessions.length).toBeGreaterThan(0);
  });
});

describe('reading', () => {
  it('refreshes authoritative session counts after prepare and close', async () => {
    const r = make();
    await r.open();
    const status = copy(real.state);
    status.authority!.prepared_count = 2;
    status.authority!.closed_count = 1;
    r.client.state.mockResolvedValue(status);
    await r.controller.refresh();
    expect(r.state.authority?.prepared_count).toBe(2);
    expect(r.state.authority?.closed_count).toBe(1);
    status.authority!.prepared_count = 1;
    status.authority!.closed_count = 2;
    await r.controller.refresh();
    expect(r.state.authority?.prepared_count).toBe(1);
    expect(r.state.authority?.closed_count).toBe(2);
  });

  it('reads a session when it is selected, and the same one once', async () => {
    const r = make();
    await r.open();
    expect(r.client.session).toHaveBeenCalledWith(SESSION);
    expect(r.state.detail?.session.id).toBe(SESSION);
    const before = r.client.session.mock.calls.length;
    r.controller.select(SESSION);
    await settle();
    expect(r.client.session.mock.calls.length).toBe(before);
    r.controller.select(null);
    expect(r.state.detail).toBeNull();
  });

  it('keeps what it read and says the link is offline when a read fails', async () => {
    const r = make();
    await r.open();
    r.client.sessions.mockRejectedValueOnce(failure('unavailable', true, 800));
    await r.controller.refresh();
    expect(r.state.readFailure?.code).toBe('unavailable');
    expect(r.state.link?.state).toBe('waiting');
    expect(r.state.sessions).not.toBeNull();
    await r.controller.refresh();
    expect(r.state.readFailure).toBeNull();
    expect(r.state.link?.state).toBe('connected');
  });

  it('answers a request made during a read with one more read, not a second one at once', async () => {
    const r = make();
    await r.controller.start();
    let release!: () => void;
    r.client.sessions.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve(real.sessions))),
    );
    const first = r.controller.refresh();
    void r.controller.refresh();
    void r.controller.refresh();
    await settle();
    expect(r.client.sessions).toHaveBeenCalledTimes(1);
    release();
    await first;
    await settle();
    expect(r.client.sessions).toHaveBeenCalledTimes(2);
  });

  it('walks pages with the cursor it was handed, back and forth', async () => {
    const r = make();
    await r.controller.start();
    const paged = copy(real.sessions);
    paged.older = paged.sessions.at(-1)!.position;
    r.client.sessions.mockResolvedValue(paged);
    await r.controller.refresh();
    await r.controller.older();
    expect(r.client.sessions).toHaveBeenLastCalledWith(paged.older);
    expect(r.state.depth).toBe(1);
    await r.controller.newer();
    expect(r.client.sessions).toHaveBeenLastCalledWith(null);
    expect(r.state.depth).toBe(0);
  });
});

describe('an action is one intent', () => {
  it('starts a runtime under the revision and generation it was shown, and waits for ready', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.open();
    r.controller.chooseMode('local_tools');
    await r.controller.chooseFolder();
    expect(r.state.composer.workspace).toBe('/home/dev/project');
    r.client.session
      .mockResolvedValueOnce(runtimeAs('stopped'))
      .mockResolvedValueOnce(runtimeAs('initializing'))
      .mockResolvedValue(real.attached);
    await r.controller.startRuntime();
    const row = runtimeAs('stopped').session;
    expect(r.client.startRuntime).toHaveBeenCalledWith(
      'intent-1',
      SESSION,
      row.runtime.revision,
      row.runtime.generation,
      'local_tools',
      '/home/dev/project',
    );
    expect(r.state.operation).toBeNull();
    expect(r.sleeps.length).toBeGreaterThan(0);
  });

  it('does not start a Local Tools runtime for a folder nobody chose', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.open();
    await r.controller.startRuntime();
    expect(r.client.startRuntime).not.toHaveBeenCalled();
    r.controller.chooseMode('inference');
    await r.controller.startRuntime();
    expect(r.client.startRuntime).toHaveBeenCalledWith(
      expect.any(String),
      SESSION,
      expect.any(Number),
      expect.any(Number),
      'inference',
      null,
    );
  });

  it('fails the start when the runtime ends in failure, and says what the authority reported', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.open();
    r.controller.chooseMode('inference');
    r.client.session.mockResolvedValue(runtimeAs('failed'));
    await r.controller.startRuntime();
    expect(r.state.operation?.status).toBe('failed');
    expect(r.state.operation?.failure?.code).toBe('runtime_unavailable');
  });

  it('retries with exactly the identity the action was made with, and only when nothing was refused', async () => {
    const r = make();
    await r.open();
    r.client.grantLease.mockRejectedValueOnce(failure('unavailable', true));
    await r.controller.grantLease();
    expect(r.state.operation?.status).toBe('failed');
    expect(r.state.operation?.failure?.retry).toBe(true);
    await r.controller.retry();
    expect(r.client.grantLease).toHaveBeenCalledTimes(2);
    const [first, second] = r.client.grantLease.mock.calls;
    expect(second).toEqual(first);
    expect(r.state.operation).toBeNull();

    r.client.grantLease.mockRejectedValueOnce(failure('permission_denied'));
    await r.controller.grantLease();
    expect(r.state.operation?.failure?.retry).toBe(false);
    await r.controller.retry();
    expect(r.client.grantLease).toHaveBeenCalledTimes(3);
    r.controller.dismissOperation();
    expect(r.state.operation).toBeNull();
  });

  it("says, while it waits, that the answer is the user's in the confirmation window", async () => {
    const r = make();
    await r.open();
    let release!: () => void;
    r.client.grantLease.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve({}))),
    );
    const grant = r.controller.grantLease();
    await settle();
    expect(r.state.operation).toMatchObject({
      kind: 'grant',
      status: 'pending',
      step: 'confirming',
    });
    release();
    await grant;
    expect(r.state.operation).toBeNull();

    // A lease that lets the runtime do nothing on the machine asks nobody.
    for (const s of ['command_execute', 'file_change'] as const)
      r.controller.toggleScope(s);
    r.client.grantLease.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve({}))),
    );
    const quiet = r.controller.grantLease();
    await settle();
    expect(r.state.operation).toMatchObject({ kind: 'grant', step: null });
    release();
    await quiet;
  });

  it('asks the user for a renewal too, and says so', async () => {
    const r = make();
    r.client.session.mockResolvedValue(real.attached);
    await r.open();
    expect(r.state.detail?.lease).not.toBeNull();
    let release!: () => void;
    r.client.renewLease.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve({}))),
    );
    const renew = r.controller.renewLease();
    await settle();
    expect(r.state.operation).toMatchObject({
      kind: 'renew',
      step: 'confirming',
    });
    release();
    await renew;
  });

  it('does not wait for ever for an answer that never comes: it says the outcome is not known, reads the authority and sends nothing again by itself', async () => {
    const r = make();
    await r.open();
    r.client.grantLease.mockImplementationOnce(() => never());
    const reads = r.client.session.mock.calls.length;
    const grant = r.controller.grantLease();
    await settle();
    expect(r.state.operation?.status).toBe('pending');
    // One bound, the longer one: the user may be reading the confirmation window.
    const [bound] = r.timers.filter((t) => !t.cancelled);
    expect(bound.ms).toBe(CONFIRM_LIMIT_MS);
    bound.fire();
    await grant;
    expect(r.state.operation?.status).toBe('failed');
    expect(r.state.operation?.failure).toEqual({
      code: 'unavailable',
      retry: true,
      retry_in_ms: null,
    });
    // What the authority holds was read again, and nothing was asked twice.
    expect(r.client.session.mock.calls.length).toBeGreaterThan(reads);
    expect(r.client.grantLease).toHaveBeenCalledTimes(1);
    expect(r.timers.every((t) => t.cancelled)).toBe(true);
  });

  it('ends a lost action that is asked again after the authority was replaced or lost the lease: it says so, reads again and asks nothing more', async () => {
    for (const refusal of ['stale', 'not_found'] as const) {
      const r = make();
      await r.open();
      r.client.grantLease.mockImplementationOnce(() => never());
      const grant = r.controller.grantLease();
      await settle();
      r.timers.find((t) => !t.cancelled)!.fire();
      await grant;
      expect(r.state.operation?.failure?.code).toBe('unavailable');
      const reads = r.client.session.mock.calls.length;
      r.client.grantLease.mockRejectedValueOnce(failure(refusal));
      await r.controller.retry();
      expect(r.state.notice).toBe('stale');
      expect(r.state.operation).toBeNull();
      expect(r.client.grantLease).toHaveBeenCalledTimes(2);
      expect(r.client.session.mock.calls.length).toBeGreaterThan(reads);
      await r.controller.retry();
      expect(r.client.grantLease).toHaveBeenCalledTimes(2);
      r.controller.dispose();
    }
  });

  it('tells a second runtime at the capacity of one apart from a busy authority and offers no retry', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.open();
    r.controller.chooseMode('inference');
    r.client.startRuntime.mockRejectedValueOnce(failure('capacity'));
    await r.controller.startRuntime();
    expect(r.state.operation?.status).toBe('failed');
    expect(r.state.operation?.failure).toEqual({
      code: 'capacity',
      retry: false,
      retry_in_ms: null,
    });
    await r.controller.retry();
    expect(r.client.startRuntime).toHaveBeenCalledTimes(1);
  });

  it('sends a lost grant again only when the user asks, with exactly the identity it had', async () => {
    const r = make();
    await r.open();
    r.client.grantLease.mockImplementationOnce(() => never());
    const grant = r.controller.grantLease();
    await settle();
    r.timers.find((t) => !t.cancelled)!.fire();
    await grant;
    expect(r.client.grantLease).toHaveBeenCalledTimes(1);
    await r.controller.retry();
    expect(r.client.grantLease).toHaveBeenCalledTimes(2);
    const [first, second] = r.client.grantLease.mock.calls;
    expect(second).toEqual(first);
    expect(r.state.operation).toBeNull();
  });

  it('drops the failure when the answer of an action called lost arrives late, and asks nothing', async () => {
    const r = make();
    await r.open();
    let arrive!: () => void;
    r.client.grantLease.mockImplementationOnce(
      () => new Promise((resolve) => (arrive = () => resolve({}))),
    );
    const grant = r.controller.grantLease();
    await settle();
    r.timers.find((t) => !t.cancelled)!.fire();
    await grant;
    expect(r.state.operation?.status).toBe('failed');
    const reads = r.client.session.mock.calls.length;
    arrive();
    await settle();
    expect(r.state.operation).toBeNull();
    expect(r.client.session.mock.calls.length).toBeGreaterThan(reads);
    expect(r.client.grantLease).toHaveBeenCalledTimes(1);
  });

  it('keeps the failure when a lost action fails late, and a second click while it waits does nothing', async () => {
    const r = make();
    await r.open();
    let fail!: () => void;
    r.client.grantLease.mockImplementationOnce(
      () => new Promise((_, reject) => (fail = () => reject(failure('stale')))),
    );
    const grant = r.controller.grantLease();
    await settle();
    await r.controller.grantLease();
    expect(r.client.grantLease).toHaveBeenCalledTimes(1);
    r.timers.find((t) => !t.cancelled)!.fire();
    await grant;
    fail();
    await settle();
    expect(r.state.operation?.status).toBe('failed');
    expect(r.state.operation?.failure?.code).toBe('unavailable');
  });

  it('gives an action that asks nobody the shorter bound, and cancels it when the answer comes', async () => {
    const r = make();
    await r.open();
    await r.controller.revokeLease();
    const [bound] = r.timers;
    expect(bound.ms).toBe(CALL_LIMIT_MS);
    expect(bound.cancelled).toBe(true);
    expect(r.state.operation).toBeNull();
  });

  it('never runs two actions at once', async () => {
    const r = make();
    await r.open();
    let release!: () => void;
    r.client.grantLease.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve({}))),
    );
    const first = r.controller.grantLease();
    await settle();
    await r.controller.renewLease();
    await r.controller.revokeLease();
    expect(r.client.renewLease).not.toHaveBeenCalled();
    expect(r.client.revokeLease).not.toHaveBeenCalled();
    release();
    await first;
  });

  it('treats a stale refusal as the end of the action: it says so, reads again and asks nothing more', async () => {
    const r = make();
    await r.open();
    const reads = r.client.session.mock.calls.length;
    r.client.stopRuntime.mockRejectedValueOnce(failure('stale'));
    await r.controller.stopRuntime();
    expect(r.state.notice).toBe('stale');
    expect(r.state.operation).toBeNull();
    expect(r.client.stopRuntime).toHaveBeenCalledTimes(1);
    expect(r.client.session.mock.calls.length).toBeGreaterThan(reads);
    r.controller.dismissNotice();
    expect(r.state.notice).toBeNull();
  });

  it('hands the lease the scopes in the protocol order and the lifetime from the closed set', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('ready'));
    await r.open();
    // Starts with all five; take two away and put one back.
    r.controller.toggleScope('file_change');
    r.controller.toggleScope('session_create');
    r.controller.toggleScope('file_change');
    r.controller.setTtl(900);
    r.controller.setTtl(7);
    await r.controller.grantLease();
    const [, fence, revision, scopes, ttl] = r.client.grantLease.mock.calls[0];
    expect(scopes).toEqual([
      'session_resume',
      'turn_start',
      'command_execute',
      'file_change',
    ]);
    expect(ttl).toBe(900);
    expect(fence).toEqual(real.attached.session.runtime.fence);
    expect(revision).toBe(real.attached.session.revision);
    // No scope, no lease.
    for (const s of [
      'session_resume',
      'turn_start',
      'command_execute',
      'file_change',
    ] as const)
      r.controller.toggleScope(s);
    await r.controller.grantLease();
    expect(r.client.grantLease).toHaveBeenCalledTimes(1);
  });

  it('binds a folder by asking again, with the same identity, while the authority is still creating it', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('ready'));
    await r.open();
    await r.controller.chooseFolder();
    r.client.createBinding
      .mockRejectedValueOnce(failure('busy', true))
      .mockRejectedValueOnce(failure('busy', true))
      .mockResolvedValueOnce({});
    await r.controller.bindWorkspace();
    expect(r.client.createBinding).toHaveBeenCalledTimes(3);
    const ids = new Set(r.client.createBinding.mock.calls.map((c) => c[0]));
    expect(ids.size).toBe(1);
    expect(r.state.operation).toBeNull();
  });

  it('gives up asking for a binding at the bound, and shows the busy authority', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('ready'));
    await r.open();
    await r.controller.chooseFolder();
    r.client.createBinding.mockImplementation(async () => {
      r.advance(10_000);
      throw failure('busy', true);
    });
    await r.controller.bindWorkspace();
    expect(r.state.operation?.failure?.code).toBe('busy');
    expect(r.client.createBinding.mock.calls.length).toBeLessThan(10);
  });

  it('starts a turn by asking again, with the same identity, while the authority proves the runtime', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('ready'));
    await r.open();
    r.controller.setDraft('hello');
    r.client.startTurn
      .mockRejectedValueOnce(failure('busy', true))
      .mockResolvedValueOnce({ id: 'turn-1' });
    await r.controller.startTurn();
    expect(r.client.startTurn).toHaveBeenCalledTimes(2);
    const ids = new Set(r.client.startTurn.mock.calls.map((c) => c[0]));
    expect(ids.size).toBe(1);
    expect(r.state.operation).toBeNull();
  });

  it('gives up asking for a turn at the bound, keeps the text and shows the busy authority', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('ready'));
    await r.open();
    r.controller.setDraft('hello');
    r.client.startTurn.mockImplementation(async () => {
      r.advance(4_000);
      throw failure('busy', true);
    });
    await r.controller.startTurn();
    expect(r.state.operation?.failure?.code).toBe('busy');
    expect(r.client.startTurn.mock.calls.length).toBeLessThan(10);
    expect(r.state.draft).toBe('hello');
  });

  it('closes a session only when asked twice, and with the fence and revision it was shown', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.open();
    r.controller.confirm('close');
    expect(r.state.confirming).toEqual({ kind: 'close' });
    await r.controller.closeSession();
    const row = runtimeAs('stopped').session;
    expect(r.client.close).toHaveBeenCalledWith(
      'intent-1',
      row.runtime.fence,
      row.revision,
    );
    expect(r.state.confirming).toBeNull();
  });

  it('creates a session and selects what the authority answered', async () => {
    const r = make();
    await r.controller.start();
    await r.controller.refresh();
    await r.controller.prepare();
    await settle();
    expect(r.client.prepare).toHaveBeenCalledWith('intent-1');
    expect(r.state.selected).toBe(SESSION);
  });
});

describe('a turn', () => {
  async function running() {
    const r = make();
    const live = detailWith(real.attached, (d) => {
      d.turns = [copy(real.reading.turn)];
    });
    r.client.session.mockResolvedValue(live);
    await r.open();
    await settle();
    return r;
  }

  it('is started under the fence and the revision on screen, and followed to its end', async () => {
    const r = make();
    await r.open();
    const completed = parseTurnReading({
      ...copy(real.reading),
      turn: {
        ...copy(real.reading.turn),
        turn: {
          ...copy(real.reading.turn.turn),
          state: 'completed',
          cancellable: false,
          ended_at: real.reading.turn.turn.updated_at,
          reason: 'completed',
        },
      },
      events: [{ sequence: '2', kind: 'text', text: ' done' }],
      next: '2',
      approval: null,
    });
    r.client.readTurn
      .mockResolvedValueOnce(real.reading)
      .mockResolvedValue(completed);
    r.controller.setDraft('write a test');
    await r.controller.startTurn();
    await settle();
    const [intent, fence, revision, prompt] = r.client.startTurn.mock.calls[0];
    expect(intent).toBe('intent-1');
    expect(fence).toEqual(real.attached.session.runtime.fence);
    expect(revision).toBe(real.attached.session.revision);
    expect(prompt).toBe('write a test');
    // The cursor of each reading is where the last one ended.
    expect(r.client.readTurn.mock.calls[0].slice(1)).toEqual([null, true]);
    expect(r.client.readTurn.mock.calls[1].slice(1)).toEqual(['1', true]);
    expect(r.state.conversation?.following).toBe(false);
    expect(r.state.draft).toBe('');
    const text = r.state.conversation?.transcript.items
      .filter((i) => i.kind === 'assistant')
      .map((i) => (i.kind === 'assistant' ? i.text : ''))
      .join('');
    expect(text).toContain('ASSISTANT_SENTINEL done');
    expect(r.state.tab).toBe('conversation');
  });

  it('keeps the draft when the authority did not take the turn', async () => {
    const r = make();
    await r.open();
    r.client.startTurn.mockRejectedValueOnce(failure('permission_denied'));
    r.controller.setDraft('keep me');
    await r.controller.startTurn();
    expect(r.state.draft).toBe('keep me');
    expect(r.state.operation?.failure?.code).toBe('permission_denied');
    expect(r.state.conversation).toBeNull();
  });

  it('does not start a turn from an empty objective', async () => {
    const r = make();
    await r.open();
    r.controller.setDraft('   ');
    await r.controller.startTurn();
    expect(r.client.startTurn).not.toHaveBeenCalled();
  });

  it('follows a turn the window did not start, so a restarted window resumes it', async () => {
    const r = await running();
    expect(r.client.readTurn).toHaveBeenCalled();
    expect(r.state.conversation?.turn).toBe(real.reading.turn.turn.id);
  });

  it('cancels with the turn revision it was shown, and goes on following', async () => {
    const r = await running();
    await settle();
    await r.controller.cancelTurn();
    expect(r.client.cancelTurn).toHaveBeenCalledWith(
      'intent-1',
      real.attached.session.runtime.fence,
      real.reading.turn.turn.id,
      real.reading.turn.turn.revision,
    );
    expect(r.state.conversation?.turn).toBe(real.reading.turn.turn.id);
  });

  it('says it is reconnecting after a failed read and loses the turn only after many', async () => {
    const r = await running();
    r.client.readTurn.mockReset();
    r.client.readTurn.mockRejectedValue(failure('unavailable', true));
    r.controller.openTurn(real.reading.turn.turn.id.replace(/.$/, '0'));
    await settle();
    await settle();
    expect(r.state.conversation?.lost).toBe(true);
    expect(r.state.conversation?.following).toBe(false);
    expect(r.client.readTurn.mock.calls.length).toBe(20);
    expect(r.sleeps.some((ms) => ms >= 500)).toBe(true);
  });

  it('loses a turn the authority does not have at once', async () => {
    const r = await running();
    r.client.readTurn.mockReset();
    r.client.readTurn.mockRejectedValue(failure('not_found'));
    r.controller.openTurn(real.reading.turn.turn.id.replace(/.$/, '1'));
    await settle();
    expect(r.state.conversation?.lost).toBe(true);
    expect(r.client.readTurn).toHaveBeenCalledTimes(1);
  });

  it('shows the gap of a stream the authority did not keep', async () => {
    const r = make();
    await r.open();
    const gapped = parseTurnReading({
      ...copy(real.reading),
      gap: true,
      events: [{ sequence: '5', kind: 'gap', text: null }],
      next: '5',
      approval: null,
    });
    r.client.readTurn.mockResolvedValue(gapped);
    r.controller.openTurn(real.reading.turn.turn.id.replace(/.$/, '2'));
    await settle();
    expect(r.state.conversation?.gap).toBe(true);
    expect(
      r.state.conversation?.transcript.items.some((i) => i.kind === 'gap'),
    ).toBe(true);
  });
});

describe('an approval', () => {
  async function pending() {
    const r = make();
    const live = detailWith(real.attached, (d) => {
      d.turns = [copy(real.reading.turn)];
      d.approval = copy(real.approval);
    });
    r.client.session.mockResolvedValue(live);
    await r.open();
    await settle();
    return r;
  }

  it('is read once for the turn that waits on it', async () => {
    const r = await pending();
    expect(r.client.readApproval).toHaveBeenCalled();
    expect(r.state.approval?.detail.approval.id).toBe(
      real.approval.approval.id,
    );
    const reads = r.client.readApproval.mock.calls.length;
    await r.controller.refresh();
    await settle();
    expect(r.client.readApproval.mock.calls.length).toBe(reads);
  });

  it('that the authority still calls pending past its end is looked at once a second, not in a loop', async () => {
    vi.useFakeTimers();
    try {
      const r = make();
      // The window's clock is past the end the approval was shown with.
      r.advance(10 * 60_000);
      const live = detailWith(real.attached, (d) => {
        d.turns = [copy(real.reading.turn)];
        d.approval = copy(real.approval);
      });
      r.client.session.mockResolvedValue(live);
      await r.open();
      await settle();
      const before = r.client.readApproval.mock.calls.length;
      for (let i = 0; i < 6; i++) {
        await vi.advanceTimersByTimeAsync(500);
        await settle();
      }
      // Three seconds: a handful of looks, not one per round trip.
      expect(r.client.readApproval.mock.calls.length - before).toBeLessThan(6);
    } finally {
      vi.useRealTimers();
    }
  });

  it('is decided with the fence on screen, the revision it was shown and one identity', async () => {
    const r = await pending();
    for (const kind of ['accept', 'decline', 'cancel'] as const) {
      r.client.decide.mockClear();
      await r.controller.decide(kind);
      expect(r.client.decide).toHaveBeenCalledWith(
        kind,
        expect.stringMatching(/^intent-/),
        real.attached.session.runtime.fence,
        real.approval.approval.id,
        real.approval.approval.revision,
      );
    }
  });

  it('is not decided when it is not decidable or no runtime is live', async () => {
    const r = make();
    const detail = detailWith(real.attached, (d) => {
      d.turns = [copy(real.reading.turn)];
      d.approval = copy(real.approval);
    });
    const undecidable = copy(real.approval);
    undecidable.approval.decidable = false;
    undecidable.command = null;
    r.client.session.mockResolvedValue(detail);
    r.client.readApproval.mockResolvedValue(parseApprovalDetail(undecidable));
    await r.open();
    await settle();
    await r.controller.decide('accept');
    expect(r.client.decide).not.toHaveBeenCalled();
  });

  it('does not wait for ever for the answer to a decision: accept has the long bound, decline the short one, and a late answer clears the failure', async () => {
    const r = await pending();
    let arrive!: () => void;
    r.client.decide.mockImplementationOnce(
      () => new Promise((resolve) => (arrive = () => resolve({}))),
    );
    const accept = r.controller.decide('accept');
    await settle();
    expect(r.state.approval?.decision?.status).toBe('pending');
    const bound = r.timers.find((t) => !t.cancelled)!;
    expect(bound.ms).toBe(CONFIRM_LIMIT_MS);
    bound.fire();
    await accept;
    expect(r.state.approval?.decision).toMatchObject({
      status: 'failed',
      failure: { code: 'unavailable', retry: true },
    });
    expect(r.client.decide).toHaveBeenCalledTimes(1);
    arrive();
    await settle();
    expect(r.state.approval?.decision).toBeNull();
    expect(r.client.decide).toHaveBeenCalledTimes(1);

    r.client.decide.mockImplementationOnce(() => never());
    const decline = r.controller.decide('decline');
    await settle();
    const [short] = r.timers.filter((t) => !t.cancelled);
    expect(short.ms).toBe(CALL_LIMIT_MS);
    short.fire();
    await decline;
    // Sent again by the user's own choice: the same decision, the same identity.
    await r.controller.retryDecision();
    expect(r.client.decide).toHaveBeenCalledTimes(3);
    expect(r.client.decide.mock.calls[2]).toEqual(
      r.client.decide.mock.calls[1],
    );
  });

  it('retries a decision that was not answered with the same identity, and not one that was refused', async () => {
    const r = await pending();
    r.client.decide.mockRejectedValueOnce(failure('unavailable', true));
    await r.controller.decide('accept');
    expect(r.state.approval?.decision?.status).toBe('failed');
    await r.controller.retryDecision();
    const [first, second] = r.client.decide.mock.calls;
    expect(second).toEqual(first);

    r.client.decide.mockRejectedValueOnce(failure('permission_denied'));
    await r.controller.decide('decline');
    expect(r.state.approval?.decision?.failure?.retry).toBe(false);
    const calls = r.client.decide.mock.calls.length;
    await r.controller.retryDecision();
    expect(r.client.decide.mock.calls.length).toBe(calls);
    r.controller.dismissDecision();
    expect(r.state.approval?.decision).toBeNull();
  });

  it('reads the approval and the session again whatever the answer', async () => {
    const r = await pending();
    const approvals = r.client.readApproval.mock.calls.length;
    const sessions = r.client.session.mock.calls.length;
    r.client.decide.mockRejectedValueOnce(failure('stale'));
    await r.controller.decide('accept');
    expect(r.state.notice).toBe('stale');
    expect(r.client.readApproval.mock.calls.length).toBeGreaterThan(approvals);
    expect(r.client.session.mock.calls.length).toBeGreaterThan(sessions);
  });

  it('reads a file of a change set page by page and the other files on request', async () => {
    const r = make();
    const raw: Raw = copy(real.approval);
    raw.approval.kind = 'file_change';
    raw.command = null;
    raw.files = 3;
    raw.file = {
      index: 0,
      path: 'a.txt',
      kind: 'update',
      move_path: null,
      bytes: 10,
      offset: 0,
      chunk: 'AAAAA',
      next: 5,
    };
    const first = parseApprovalDetail(raw);
    const page = copy(raw);
    page.file.offset = 5;
    page.file.chunk = 'BBBBB';
    page.file.next = null;
    const other = (index: number) => {
      const o = copy(raw);
      o.file = {
        ...o.file,
        index,
        path: `f${index}.txt`,
        offset: 0,
        chunk: 'x',
        next: null,
        bytes: 1,
      };
      return parseApprovalDetail(o);
    };
    r.client.readApproval.mockImplementation(async (_id, file, offset) =>
      file === null || file === 0
        ? offset === 5
          ? parseApprovalDetail(page)
          : first
        : other(file as number),
    );
    const live = detailWith(real.attached, (d) => {
      d.turns = [copy(real.reading.turn)];
      d.approval = copy(raw);
    });
    r.client.session.mockResolvedValue(live);
    await r.open();
    await settle();
    expect(r.state.approval?.files.map((f) => f.index)).toEqual([0, 1, 2]);
    await r.controller.moreOfFile(0);
    expect(r.state.approval?.files[0].text).toBe('AAAAABBBBB');
    expect(r.state.approval?.files[0].next).toBeNull();
    expect(r.client.readApproval).toHaveBeenCalledWith(
      expect.any(String),
      0,
      5,
    );
  });
});

describe('the authority', () => {
  it('is replaced: everything of the old one is dropped, once, with a notice', async () => {
    const r = make();
    const live = detailWith(real.attached, (d) => {
      d.turns = [copy(real.reading.turn)];
      d.approval = copy(real.approval);
    });
    r.client.session.mockResolvedValue(live);
    await r.open();
    await settle();
    expect(r.state.conversation).not.toBeNull();
    expect(r.state.approval).not.toBeNull();
    const replaced = copy(real.sessions);
    replaced.authority = OTHER_AUTHORITY as typeof replaced.authority;
    r.client.sessions.mockResolvedValue(replaced);
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.controller.refresh();
    expect(r.state.notice).toBe('authority_changed');
    expect(r.state.conversation).toBeNull();
    expect(r.state.approval).toBeNull();
    expect(r.state.operation).toBeNull();
    r.controller.dismissNotice();
    expect(r.state.notice).toBeNull();
    // The selection is read again, not guessed.
    expect(r.client.session).toHaveBeenCalled();
  });

  it('lets a turn that ends being followed see the replacement and stop', async () => {
    const r = make();
    await r.open();
    const other = parseTurnReading({
      ...copy(real.reading),
      authority: OTHER_AUTHORITY,
    });
    r.client.readTurn.mockResolvedValue(other);
    r.controller.openTurn(real.reading.turn.turn.id.replace(/.$/, '3'));
    await settle();
    expect(r.state.notice).toBe('authority_changed');
  });

  it('is reached again on request, without waiting out the delay', async () => {
    const r = make();
    await r.open();
    r.client.sessions.mockRejectedValueOnce(failure('unavailable', true, 5000));
    await r.controller.refresh();
    expect(r.state.link?.state).toBe('waiting');
    await r.controller.reconnect();
    expect(r.client.reconnect).toHaveBeenCalled();
    expect(r.state.link?.state).toBe('connected');
  });
});

describe('waiting for the stream', () => {
  it('reads again when the authority commits something, and not otherwise', async () => {
    const r = make();
    await r.controller.start();
    const wakes: ((value: WatchInfo) => void)[] = [];
    for (let i = 0; i < 2; i++)
      r.client.wait.mockImplementationOnce(
        () => new Promise<WatchInfo>((resolve) => wakes.push(resolve)),
      );
    r.controller.setVisible(true);
    await settle();
    const stream = real.state.authority!.stream_id;
    expect(r.client.wait).toHaveBeenCalledWith(
      stream,
      real.state.authority!.last_sequence,
      20_000,
    );
    const reads = r.client.sessions.mock.calls.length;
    // Nothing was committed: the wait ended empty, and nothing is read.
    wakes[0]({
      schema_version: 1,
      authority: AUTHORITY,
      stream,
      head: '99',
      changed: false,
    });
    await settle();
    expect(r.client.sessions.mock.calls.length).toBe(reads);
    expect(r.client.wait).toHaveBeenLastCalledWith(stream, '99', 20_000);
    wakes[1]({
      schema_version: 1,
      authority: AUTHORITY,
      stream,
      head: '100',
      changed: true,
    });
    await settle();
    expect(r.client.sessions.mock.calls.length).toBeGreaterThan(reads);
    // The cursor it waits from is the head it was told.
    expect(r.client.wait).toHaveBeenLastCalledWith(stream, '100', 20_000);
  });

  it('asks the authority where it is after a failed wait, and does not wait while hidden', async () => {
    const r = make();
    await r.controller.start();
    r.client.wait.mockRejectedValueOnce(failure('unavailable', true, 600));
    r.controller.setVisible(true);
    await settle();
    // It waited the delay the link named, then asked where the authority is and read again.
    expect(r.sleeps).toContain(600);
    expect(r.client.state.mock.calls.length).toBeGreaterThan(1);
    expect(r.state.link?.state).toBe('connected');
    r.controller.setVisible(false);
    const waits = r.client.wait.mock.calls.length;
    await settle();
    expect(r.client.wait.mock.calls.length).toBe(waits);
  });

  it('stops for good on an authority it cannot speak to', async () => {
    const r = make();
    await r.controller.start();
    r.client.wait.mockRejectedValue(failure('incompatible'));
    r.controller.setVisible(true);
    await settle();
    expect(r.client.wait).toHaveBeenCalledTimes(1);
  });
});

describe('models and history', () => {
  it('lists the models of a launch once, and again for another', async () => {
    const r = make();
    await r.open();
    r.controller.setTab('models');
    await settle();
    expect(r.client.models).toHaveBeenCalledWith(
      real.attached.session.runtime.fence,
    );
    expect(r.state.models.status).toBe('ready');
    r.controller.setTab('overview');
    r.controller.setTab('models');
    await settle();
    expect(r.client.models).toHaveBeenCalledTimes(1);
    const next = detailWith(real.attached, (d) => {
      d.session.runtime.generation = 2;
      d.session.runtime.fence.generation = 2;
    });
    r.client.session.mockResolvedValue(next);
    await r.controller.refresh();
    await settle();
    expect(r.client.models).toHaveBeenCalledTimes(2);
  });

  it('lists nothing for a runtime that is not ready', async () => {
    const r = make();
    r.client.session.mockResolvedValue(runtimeAs('stopped'));
    await r.open();
    r.controller.setTab('models');
    await settle();
    expect(r.client.models).not.toHaveBeenCalled();
    expect(r.state.models.status).toBe('idle');
  });

  it('shows why the models could not be read', async () => {
    const r = make();
    await r.open();
    r.client.models.mockRejectedValueOnce(failure('busy', true));
    r.controller.setTab('models');
    await settle();
    expect(r.state.models.status).toBe('failed');
    await r.controller.loadModels();
    expect(r.state.models.status).toBe('ready');
  });

  it('reads history newest first and further back with the position it was handed', async () => {
    const r = make();
    await r.open();
    r.controller.setTab('history');
    await settle();
    expect(r.client.audit).toHaveBeenCalledWith(null);
    const page = r.state.history.page!.page;
    if (page.older) {
      await r.controller.loadHistory(true);
      expect(r.client.audit).toHaveBeenLastCalledWith({
        stream_id: page.stream,
        before: page.older,
      });
      expect(r.state.history.depth).toBe(1);
    }
  });

  it('starts from the newest window when the position it held moved', async () => {
    const r = make();
    await r.open();
    const older = copy(real.audit);
    older.page.older = older.page.events.at(-1)!.sequence;
    r.client.audit.mockResolvedValueOnce(older);
    await r.controller.loadHistory(false);
    r.client.audit
      .mockRejectedValueOnce(failure('stale'))
      .mockResolvedValueOnce(real.audit);
    await r.controller.loadHistory(true);
    expect(r.client.audit).toHaveBeenLastCalledWith(null);
    expect(r.state.history.status).toBe('ready');
  });

  it('is never read while hidden', async () => {
    const r = make();
    await r.controller.start();
    await settle();
    expect(r.client.audit).not.toHaveBeenCalled();
    expect(r.client.wait).not.toHaveBeenCalled();
    void never;
  });
});

describe('a change set', () => {
  it('takes only the file that was asked for, once, whatever the authority answers', async () => {
    const r = make();
    const raw: Raw = copy(real.approval);
    raw.approval.kind = 'file_change';
    raw.command = null;
    raw.files = 3;
    raw.file = {
      index: 0,
      path: 'a.txt',
      kind: 'update',
      move_path: null,
      bytes: 1,
      offset: 0,
      chunk: 'x',
      next: null,
    };
    // Every read is answered with the first file: the others were asked for and never given.
    r.client.readApproval.mockResolvedValue(parseApprovalDetail(raw));
    const live = detailWith(real.attached, (d) => {
      d.turns = [copy(real.reading.turn)];
      d.approval = copy(raw);
    });
    r.client.session.mockResolvedValue(live);
    await r.open();
    await settle();
    expect(r.state.approval?.files.map((f) => f.index)).toEqual([0]);
    await r.controller.moreFiles();
    expect(r.state.approval?.files.map((f) => f.index)).toEqual([0]);
  });
});
