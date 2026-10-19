import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  parseFailure,
  parseLauncher,
  parseSession,
  parseStartReply,
} from '../src/lib/contracts/native';
import { NativeController } from '../src/lib/features/native/controller';
import {
  folderName,
  formatBytes,
  groupByProject,
  sharedFolders,
  stateLabel,
} from '../src/lib/features/native/present';
import { fakeClient, launcher, reply, session } from './native-harness';

const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};

describe('the contract', () => {
  it('believes only a session that has every field and is on the native plane', () => {
    expect(session().plane).toBe('native');
    const without: Record<string, unknown> = { ...session() };
    delete without.plane;
    expect(() => parseSession(without)).toThrow();
    expect(() => parseSession({ ...session(), plane: 'managed' })).toThrow();
    expect(() => parseSession({ ...session(), state: 'finished' })).toThrow();
    expect(() => parseSession({ ...session(), agent_id: '/bin/sh' })).toThrow();
    expect(() => parseSession({ ...session(), pid: -1 })).toThrow();
    expect(() =>
      parseLauncher({ agents: 'x', shell: null, projects: [] }),
    ).toThrow();
  });
  it('takes a start reply only when the terminal is the one the session names', () => {
    expect(reply().terminal.terminal_id).toBe(reply().session.terminal_id);
    expect(() =>
      parseStartReply({
        ...reply(),
        terminal: {
          ...reply().terminal,
          terminal_id: '77777777-7777-4777-8777-777777777777',
        },
      }),
    ).toThrow();
  });
  it('shows a failure as a short word and bounded reasons, never the raw error', () => {
    expect(
      parseFailure({ code: 'profile_not_applicable', reasons: ['a', 3, 'b'] }),
    ).toEqual({
      code: 'profile_not_applicable',
      reasons: ['a', 'b'],
    });
    expect(parseFailure('boom').code).toBe('internal');
    expect(parseFailure({ code: '../../etc' }).code).toBe('internal');
  });
});

describe('what the overview says', () => {
  it('names each state in words that do not claim more than is known', () => {
    expect(stateLabel(session({ state: 'running' }))).toEqual({
      text: 'Active',
      tone: 'ok',
    });
    expect(
      stateLabel(session({ state: 'requested', restart_of: 'x' })).text,
    ).toBe('Restarting');
    expect(stateLabel(session({ state: 'requested' })).text).toBe('Starting');
    expect(stateLabel(session({ state: 'exited', exit_code: 0 })).tone).toBe(
      'idle',
    );
    expect(stateLabel(session({ state: 'exited', exit_code: 3 })).text).toBe(
      'Failed (3)',
    );
    expect(stateLabel(session({ state: 'stopped' })).text).toBe('Stopped');
    // After a crash of the window a session is lost, never "exited".
    expect(stateLabel(session({ state: 'lost' })).text).toBe('Lost');
  });
  it('groups by project and warns where two sessions run on one folder', () => {
    const a = session({ id: '1'.repeat(32), project: '/p', started_at: 5 });
    const b = session({ id: '2'.repeat(32), project: '/p', started_at: 9 });
    const c = session({
      id: '3'.repeat(32),
      project: '/q',
      state: 'stopped',
      started_at: 1,
    });
    expect(groupByProject([a, b, c]).map((g) => g.project)).toEqual([
      '/p',
      '/q',
    ]);
    expect(groupByProject([a, b])[0]!.sessions.map((s) => s.id)).toEqual([
      b.id,
      a.id,
    ]);
    expect([...sharedFolders([a, b, c])]).toEqual(['/p']);
    expect(folderName('/home/u/work/app')).toBe('app');
    expect(formatBytes(300 * 1024 * 1024)).toBe('300 MB');
  });
});

describe('the controller', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('starts an agent once at a time, hands the terminal to the window and says why not', async () => {
    const client = fakeClient();
    const started = vi.fn();
    const controller = new NativeController(() => undefined, started, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(client.launcher).toHaveBeenCalled();
    const first = controller.startAgent('claude_code', '/home/u', null);
    // A second start while one is in progress is not sent.
    expect(await controller.startAgent('claude_code', '/home/u', null)).toBe(
      false,
    );
    expect(await first).toBe(true);
    expect(client.start).toHaveBeenCalledTimes(1);
    expect(client.start).toHaveBeenCalledWith('claude_code', '/home/u', null);
    expect(started).toHaveBeenCalledWith(
      expect.objectContaining({ session: expect.anything() }),
    );
    controller.dispose();
  });

  it('keeps the failure of a refused start for the page to show', async () => {
    const { NativeCallError } =
      await import('../src/lib/desktop/native-client');
    const client = fakeClient({
      start: vi.fn(async () => {
        throw new NativeCallError({
          code: 'profile_not_applicable',
          reasons: ['Codex hooks'],
        });
      }),
    });
    let last = new NativeController(() => undefined).state();
    const controller = new NativeController(
      (s) => (last = s),
      undefined,
      client,
    );
    expect(await controller.startAgent('codex', '/home/u', 'x')).toBe(false);
    expect(last.failure).toEqual({
      code: 'profile_not_applicable',
      reasons: ['Codex hooks'],
    });
    expect(last.starting).toBe(false);
    controller.dispose();
  });

  it('reads the registry with resources only while the overview is shown, and slowly while something runs', async () => {
    const running = [session()];
    const client = fakeClient({ sessions: vi.fn(async () => running) });
    const controller = new NativeController(
      () => undefined,
      undefined,
      client,
      {
        overview: 100,
        background: 400,
      },
    );
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(client.sessions).toHaveBeenLastCalledWith(false); // the first read, before it is known to be shown
    await vi.advanceTimersByTimeAsync(100);
    expect(client.sessions).toHaveBeenLastCalledWith(true);
    // Hidden, with a session running: still read, slower, and without sampling.
    controller.setVisible(false);
    vi.mocked(client.sessions).mockClear();
    await vi.advanceTimersByTimeAsync(399);
    expect(client.sessions).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(2);
    expect(client.sessions).toHaveBeenCalledWith(false);
    controller.dispose();
  });

  it('reads nothing while hidden when nothing runs', async () => {
    const client = fakeClient();
    const controller = new NativeController(
      () => undefined,
      undefined,
      client,
      {
        overview: 100,
        background: 100,
      },
    );
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    controller.setVisible(false);
    vi.mocked(client.sessions).mockClear();
    await vi.advanceTimersByTimeAsync(5000);
    expect(client.sessions).not.toHaveBeenCalled();
    controller.dispose();
  });

  it('refuses nothing silently: a failed read of the launcher is a failed phase', async () => {
    const client = fakeClient({
      launcher: vi.fn(async () => {
        throw new Error('down');
      }),
    });
    let last = new NativeController(() => undefined).state();
    const controller = new NativeController(
      (s) => (last = s),
      undefined,
      client,
    );
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    await settle();
    expect(last.phase).toBe('failed');
    expect(last.failure?.code).toBe('internal');
    controller.dispose();
  });

  it('uses the launcher fixture shape', () => {
    expect(launcher().projects[0]!.home).toBe(true);
  });
});
