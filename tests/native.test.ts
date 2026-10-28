import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  parseFailure,
  parseLauncher,
  parsePreview,
  parseSession,
  parseStartReply,
  type Launcher,
  type NativeSession,
} from '../src/lib/contracts/native';
import { NativeController } from '../src/lib/features/native/controller';
import { NativeCallError } from '../src/lib/desktop/native-client';
import {
  failureText,
  restorable,
  folderName,
  formatBytes,
  groupByProject,
  modelFacts,
  sharedFolders,
  stateLabel,
  START_UNRECORDED,
  STOP_UNRECORDED,
  switchHint,
} from '../src/lib/features/native/present';
import {
  fakeClient,
  launcher,
  launcherWithM9,
  profiles,
  reply,
  session,
} from './native-harness';
import type { ConfigItem } from '../src/lib/contracts/configuration';

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
  it('treats missing capability metadata as unverified and validates evidence shapes', () => {
    expect(launcher().agents[0]?.profile_capability.status).toBe('unverified');
    const parsed = parseLauncher({
      ...launcher(),
      agents: [
        {
          ...launcher().agents[0],
          profile_capability: {
            status: 'syntax_verified',
            qualified_version: '2.1.309 (Claude Code)',
            checked_on: '2026-10-23',
            operations: ['model via --model'],
            interface: '--model and --effort',
            format: 'argv flags',
            scope: 'one process',
            reload_semantics: 'Not tested',
            source: 'installed CLI --help',
            limitation: 'Syntax only; effective value not reported.',
          },
        },
      ],
    });
    expect(parsed.agents[0]?.profile_capability.status).toBe('syntax_verified');
    expect(() =>
      parseLauncher({
        ...launcher(),
        agents: [
          {
            ...launcher().agents[0],
            profile_capability: { status: 'verified' },
          },
        ],
      }),
    ).toThrow();
  });
  it('reads capability warnings and tolerates a pre-M2 preview without them', () => {
    const oldPreview = {
      applied: [],
      with_cost: [],
      not_applicable: [],
      can_start: true,
      through_shell: true,
    };
    expect(parsePreview(oldPreview).warnings).toEqual([]);
    expect(parsePreview(oldPreview).plan_token).toBeNull();
    expect(parsePreview(oldPreview).provider_version).toBeNull();
    expect(
      parsePreview({ ...oldPreview, warnings: ['Exact build is unverified.'] })
        .warnings,
    ).toEqual(['Exact build is unverified.']);
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
    expect(
      stateLabel(
        session({ state: 'exited', exit_code: 1, exit_signal: 'Killed' }),
      ).text,
    ).toBe('Ended by a signal');
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
  it('explains when a profile override was refused for an unverified provider build', () => {
    expect(failureText('provider_version_unverified')).toContain(
      'exact provider build is not verified',
    );
    expect(failureText('provider_version_unverified')).toContain(
      'was not launched',
    );
    expect(failureText('profile_preview_stale')).toContain(
      'Review the new preview',
    );
  });
});

describe('restoring a workspace', () => {
  it('offers what was lost and has no successor, and starts each again as a new session', async () => {
    const lost = session({
      id: '1'.repeat(32),
      state: 'lost',
      terminal_id: null,
      pid: null,
    });
    const already = session({
      id: '2'.repeat(32),
      state: 'lost',
      terminal_id: null,
      pid: null,
    });
    const successor = session({ id: '3'.repeat(32), restart_of: already.id });
    const stopped = session({
      id: '4'.repeat(32),
      state: 'stopped',
      terminal_id: null,
      pid: null,
    });
    expect(
      restorable([lost, already, successor, stopped]).map((s) => s.id),
    ).toEqual([lost.id]);
  });
  it('restores in order and stops at the first refusal', async () => {
    vi.useFakeTimers();
    const lost = ['1', '2', '3'].map((n) =>
      session({
        id: n.repeat(32),
        state: 'lost',
        terminal_id: null,
        pid: null,
      }),
    );
    const { NativeCallError } =
      await import('../src/lib/desktop/native-client');
    let calls = 0;
    const client = fakeClient({
      sessions: vi.fn(async () => lost),
      restart: vi.fn(async () => {
        calls += 1;
        if (calls === 2)
          throw new NativeCallError({ code: 'limit_reached', reasons: [] });
        return reply({ id: 'f'.repeat(32) });
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(await controller.restoreLost()).toBe(1);
    expect(client.restart).toHaveBeenCalledTimes(2);
    expect(vi.mocked(client.restart).mock.calls[0]![0]).toBe('1'.repeat(32));
    controller.dispose();
    vi.useRealTimers();
  });
});

describe('tidying the registry', () => {
  it('removes the records of what is not running and nothing else', async () => {
    vi.useFakeTimers();
    const sessions = [
      session({ id: '1'.repeat(32), state: 'running' }),
      session({
        id: '2'.repeat(32),
        state: 'lost',
        terminal_id: null,
        pid: null,
      }),
      session({
        id: '3'.repeat(32),
        state: 'exited',
        exit_code: 0,
        terminal_id: null,
        pid: null,
      }),
    ];
    const client = fakeClient({ sessions: vi.fn(async () => sessions) });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    await controller.forgetFinished();
    expect(vi.mocked(client.forget).mock.calls.map((c) => c[0])).toEqual([
      '2'.repeat(32),
      '3'.repeat(32),
    ]);
    controller.dispose();
    vi.useRealTimers();
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
    expect(client.start).toHaveBeenCalledWith(
      'claude_code',
      '/home/u',
      null,
      null,
      null,
    );
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

describe('the model of a native session', () => {
  it('keeps the effort asked for and the effort the files hold as two facts, per agent’s own key', () => {
    const p = structuredClone(profiles()[0]);
    p.id = 'p'.repeat(32);
    p.template = false;
    p.current = 3;
    p.body.effort = 'high';
    const claude = session({ profile: { id: p.id, name: 'x', version: 3 } });
    const effort = (key: string, provider: string, text: string): ConfigItem =>
      ({
        id: 'e'.repeat(32),
        provider,
        source: 'claude_user_settings',
        scope: 'global',
        kind: 'model_preference',
        name: key,
        persisted: 'present_enabled',
        effective: 'documented',
        session_use: 'unknown',
        not_writable: 'not_in_this_phase',
        detail: { kind: 'text', value: { text, found: [] } },
        hash: null,
      }) as ConfigItem;
    const facts = modelFacts(
      claude,
      [p],
      [effort('effortLevel', 'claude_code', 'low')],
    );
    expect(facts.requestedEffort).toBe('high');
    expect(facts.configuredEffort).toBe('low');
    // Another agent's key is not this agent's setting, and nothing is filled in for it.
    expect(
      modelFacts(
        claude,
        [p],
        [effort('model_reasoning_effort', 'codex', 'xhigh')],
      ).configuredEffort,
    ).toBe('not set in its files');
    expect(modelFacts(claude, [p], null).configuredEffort).toContain(
      'not read yet',
    );
    expect(facts.inUse).toBe('not reported by the agent');
  });
  it('says in plain words that an older terminal host cannot keep an effort, and a code it has no words for is never shown raw', () => {
    expect(failureText('host_outdated')).toContain('older than this window');
    expect(failureText('host_outdated')).toContain('preview-bound profiles');
    expect(failureText('profile_effort_invalid')).toContain('letters, digits');
    expect(failureText('some_unknown_code')).not.toContain('some_unknown_code');
  });
  it('offers Claude Code’s own command to change it, and none for Codex, whose is not verified', () => {
    expect(switchHint('claude_code')).toContain('/model');
    expect(switchHint('claude_code')).toContain(
      'saves your choice as its default',
    );
    expect(switchHint('codex')).toContain('not verified');
    expect(switchHint('codex')).not.toContain('/model');
  });
  const saved = () => {
    const p = structuredClone(profiles()[0]);
    p.id = 'p'.repeat(32);
    p.template = false;
    p.current = 3;
    p.body.model = 'sonnet';
    return p;
  };
  const modelItem = (text: string, provider = 'claude_code'): ConfigItem =>
    ({
      id: 'b'.repeat(32),
      provider,
      source: 'claude_user_settings',
      scope: 'global',
      kind: 'model_preference',
      name: 'model',
      persisted: 'present_enabled',
      effective: 'documented',
      session_use: 'unknown',
      not_writable: 'not_in_this_phase',
      detail: { kind: 'text', value: { text, found: [] } },
      hash: null,
    }) as ConfigItem;

  it('keeps what was asked for, what the files say and what is in use as three facts', () => {
    const p = saved();
    const s = session({ profile: { id: p.id, name: 'x', version: 3 } });
    const facts = modelFacts(s, [p], [modelItem('opus')]);
    expect(facts.requested).toBe('sonnet');
    expect(facts.configured).toBe('opus');
    // Never filled from either of the other two.
    expect(facts.inUse).toBe('not reported by the agent');
    expect(facts.inUse).not.toContain('sonnet');
    expect(facts.inUse).not.toContain('opus');
  });

  it('says so when nothing was asked, when the profile moved on or is gone, and when nothing was read', () => {
    const p = saved();
    expect(modelFacts(session(), [p], null).requested).toContain('own choice');
    expect(modelFacts(session(), [p], null).configured).toContain(
      'not read yet',
    );
    const moved = session({ profile: { id: p.id, name: 'x', version: 2 } });
    expect(modelFacts(moved, [p], []).requested).toContain('changed since');
    expect(modelFacts(moved, [], []).requested).toContain('no longer exists');
    expect(modelFacts(session(), [], []).configured).toBe(
      'not set in its files',
    );
    // Another agent's setting is not this agent's.
    expect(
      modelFacts(session(), [], [modelItem('gpt-6', 'codex')]).configured,
    ).toBe('not set in its files');
  });
});

describe('a stop whose M9 record could not be written', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const storeFailure = () =>
    new NativeCallError({ code: 'store_unavailable', reasons: [] });
  const runningOpencode = (): NativeSession =>
    session({ agent_id: 'opencode', agent_name: 'OpenCode' });
  const stoppedOpencode = (): NativeSession =>
    session({
      agent_id: 'opencode',
      agent_name: 'OpenCode',
      state: 'stopped',
      ended_at: 1_700_000_001,
      terminal_id: null,
      pid: null,
    });
  const id = 'a'.repeat(32);

  it('reads the launcher and the sessions again, blocks OpenCode, and never stops twice', async () => {
    let stopped = false;
    const client = fakeClient({
      readiness: vi.fn(async () =>
        launcherWithM9(stopped ? 'storage_unavailable' : 'ready'),
      ),
      sessions: vi.fn(async () => [
        stopped ? stoppedOpencode() : runningOpencode(),
      ]),
      stop: vi.fn(async () => {
        stopped = true;
        throw storeFailure();
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    await controller.stop(id);
    await vi.advanceTimersByTimeAsync(0);

    const state = controller.state();
    expect(client.stop).toHaveBeenCalledTimes(1);
    expect(state.sessions[0]?.state).toBe('stopped');
    expect(state.launcher?.m9_native_status).toBe('storage_unavailable');
    expect(state.m9Blocked).toBe(true);
    expect(state.failure).toEqual({
      code: 'store_unavailable',
      reasons: [STOP_UNRECORDED],
    });
    expect(await controller.startAgent('opencode', '/home/u', null)).toBe(
      false,
    );
    expect(client.start).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(client.stop).toHaveBeenCalledTimes(1);
    expect(client.restart).not.toHaveBeenCalled();
    controller.dispose();
  });

  it('keeps M9 waiting while the launcher cannot be read, and reads it again on request', async () => {
    let failing = false;
    const client = fakeClient({
      readiness: vi.fn(async () => {
        if (failing) throw new Error('the desktop did not answer');
        return launcherWithM9('ready');
      }),
      stop: vi.fn(async () => {
        failing = true;
        throw storeFailure();
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    await controller.stop(id);
    await vi.advanceTimersByTimeAsync(0);

    expect(controller.state().m9Uncertain).toBe(true);
    expect(controller.state().m9Blocked).toBe(true);
    expect(controller.state().failure?.reasons).toEqual([STOP_UNRECORDED]);
    expect(await controller.startAgent('opencode', '/home/u', null)).toBe(
      false,
    );
    expect(client.start).not.toHaveBeenCalled();

    failing = false;
    await controller.refreshLauncher();
    expect(controller.state().m9Uncertain).toBe(false);
    expect(controller.state().m9Blocked).toBe(false);
    expect(await controller.startAgent('opencode', '/home/u', null)).toBe(true);
    controller.dispose();
  });

  it('leaves Claude Code available in the same degraded store', async () => {
    const client = fakeClient({
      readiness: vi.fn(async () => launcherWithM9('storage_unavailable')),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);

    expect(controller.state().m9Blocked).toBe(true);
    expect(await controller.startAgent('claude_code', '/home/u', null)).toBe(
      true,
    );
    expect(client.start).toHaveBeenCalledTimes(1);
    controller.dispose();
  });

  it('changes nothing when the stop is recorded', async () => {
    let stopped = false;
    const client = fakeClient({
      readiness: vi.fn(async () => launcherWithM9('ready')),
      sessions: vi.fn(async () => [
        stopped ? stoppedOpencode() : runningOpencode(),
      ]),
      stop: vi.fn(async () => {
        stopped = true;
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    await controller.stop(id);
    await vi.advanceTimersByTimeAsync(0);

    expect(controller.state().failure).toBeNull();
    expect(controller.state().m9Blocked).toBe(false);
    expect(controller.state().sessions[0]?.state).toBe('stopped');
    expect(await controller.startAgent('opencode', '/home/u', null)).toBe(true);
    expect(client.stop).toHaveBeenCalledTimes(1);
    controller.dispose();
  });

  it('lets the newest launcher read decide, even when an older answer arrives last', async () => {
    const answers: Array<(l: Launcher) => void> = [];
    const client = fakeClient({
      readiness: vi.fn(
        () => new Promise<Launcher>((resolve) => answers.push(resolve)),
      ),
      stop: vi.fn(async () => {
        throw storeFailure();
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    answers[0]!(launcherWithM9('ready'));
    await vi.advanceTimersByTimeAsync(0);

    const older = controller.refreshLauncher();
    const stopping = controller.stop(id);
    await vi.advanceTimersByTimeAsync(0);
    expect(answers).toHaveLength(3);
    answers[2]!(launcherWithM9('storage_unavailable'));
    await vi.advanceTimersByTimeAsync(0);
    answers[1]!(launcherWithM9('ready'));
    await Promise.all([older, stopping]);
    await vi.advanceTimersByTimeAsync(0);

    expect(controller.state().launcher?.m9_native_status).toBe(
      'storage_unavailable',
    );
    expect(controller.state().m9Blocked).toBe(true);
    controller.dispose();
  });

  it('reads the launcher again when polling sees a session end by itself', async () => {
    let ended = false;
    const client = fakeClient({
      readiness: vi.fn(async () =>
        launcherWithM9(ended ? 'storage_unavailable' : 'ready'),
      ),
      sessions: vi.fn(async () => [
        ended ? stoppedOpencode() : runningOpencode(),
      ]),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(client.readiness).toHaveBeenCalledTimes(1);

    ended = true;
    await vi.advanceTimersByTimeAsync(2000);
    await vi.advanceTimersByTimeAsync(0);
    expect(client.readiness).toHaveBeenCalledTimes(2);
    expect(controller.state().m9Blocked).toBe(true);
    controller.dispose();
  });

  it('reads the sessions again when a read was in flight during the stop', async () => {
    const answers: Array<(s: NativeSession[]) => void> = [];
    const client = fakeClient({
      readiness: vi.fn(async () => launcherWithM9('ready')),
      sessions: vi.fn(
        () => new Promise<NativeSession[]>((resolve) => answers.push(resolve)),
      ),
      stop: vi.fn(async () => {
        throw storeFailure();
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    answers[0]!([runningOpencode()]);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(2000);
    expect(answers).toHaveLength(2);

    const stopping = controller.stop(id);
    await vi.advanceTimersByTimeAsync(0);
    // The read that was running answers with what was true before the stop; the next one is
    // requested only once it has answered, so the stop is not lost.
    expect(answers).toHaveLength(2);
    answers[1]!([runningOpencode()]);
    await vi.advanceTimersByTimeAsync(0);
    expect(answers).toHaveLength(3);
    answers[2]!([stoppedOpencode()]);
    await stopping;
    await vi.advanceTimersByTimeAsync(0);

    expect(controller.state().sessions[0]?.state).toBe('stopped');
    controller.dispose();
  });
});

describe('a start whose M9 record could not be written, and the first load', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const storeFailure = () =>
    new NativeCallError({ code: 'store_unavailable', reasons: [] });

  it('is not retried, says a process may exist, and leaves M9 waiting for the store', async () => {
    let degraded = false;
    const client = fakeClient({
      readiness: vi.fn(async () =>
        launcherWithM9(degraded ? 'storage_unavailable' : 'ready'),
      ),
      start: vi.fn(async () => {
        degraded = true;
        throw storeFailure();
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);

    expect(await controller.startAgent('claude_code', '/home/u', null)).toBe(
      false,
    );
    await vi.advanceTimersByTimeAsync(0);
    expect(controller.state().failure).toEqual({
      code: 'store_unavailable',
      reasons: [START_UNRECORDED],
    });
    expect(controller.state().m9Blocked).toBe(true);
    expect(await controller.startAgent('opencode', '/home/u', null)).toBe(
      false,
    );
    expect(client.start).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(client.start).toHaveBeenCalledTimes(1);
    controller.dispose();
  });

  it('treats a restart whose record failed the same way, and reads the launcher again', async () => {
    let launcherReads = 0;
    const client = fakeClient({
      readiness: vi.fn(async () => {
        launcherReads += 1;
        return launcherWithM9('ready');
      }),
      sessions: vi.fn(async () => [session()]),
      restart: vi.fn(async () => {
        throw storeFailure();
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    const readsBefore = launcherReads;

    expect(await controller.restart('a'.repeat(32))).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(controller.state().failure?.reasons).toEqual([START_UNRECORDED]);
    expect(client.restart).toHaveBeenCalledTimes(1);
    expect(launcherReads).toBeGreaterThan(readsBefore);
    controller.dispose();
  });

  it('lets a slow first answer never restore a readiness that a later read withdrew', async () => {
    const answers: Array<(l: Launcher) => void> = [];
    let reads = 0;
    const client = fakeClient({
      readiness: vi.fn(() => {
        reads += 1;
        if (reads === 1)
          return new Promise<Launcher>((resolve) => answers.push(resolve));
        return Promise.resolve(launcherWithM9('storage_unavailable'));
      }),
    });
    const controller = new NativeController(() => undefined, undefined, client);
    controller.setVisible(true);
    await vi.advanceTimersByTimeAsync(0);
    await controller.refreshLauncher();
    expect(controller.state().m9Blocked).toBe(true);

    answers[0]!(launcherWithM9('ready'));
    await vi.advanceTimersByTimeAsync(0);
    expect(controller.state().phase).toBe('ready');
    expect(controller.state().launcher?.m9_native_status).toBe(
      'storage_unavailable',
    );
    expect(controller.state().m9Blocked).toBe(true);
    controller.dispose();
  });
});
