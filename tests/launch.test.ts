import { describe, expect, it, vi } from 'vitest';
import {
  isFullAccess,
  openLaunch,
  type LaunchDeps,
} from '../src/lib/features/terminal/launch';
import type { Launch } from '../src/lib/desktop/launch-client';
import type { Profile, StartReply } from '../src/lib/contracts/native';

const profile = (
  id: string,
  name: string,
  permission: string | null,
  applies = ['claude_code', 'codex'],
) =>
  ({
    id,
    template: false,
    current: 1,
    versions: [],
    body: { name, applies_to: applies, permission },
    from: {},
    refers_to_missing: false,
  }) as unknown as Profile;
const PROFILES = [
  profile('p-safe', 'Strict', 'ask'),
  profile('p-full', 'Full access (bypass)', 'bypass'),
  profile('p-claude-only', 'Claude only', 'edits', ['claude_code']),
];
const session = (
  over: Partial<Extract<Launch, { kind: 'session' }>> = {},
): Launch => ({
  id: 'codex',
  name: 'Codex',
  directory: '/home/me',
  kind: 'session',
  agent: 'codex',
  profile: 'Full access (bypass)',
  before: ['scan'],
  ...over,
});
function deps(
  launch: Launch,
  before = [{ command: 'scan', result: 'ok' }],
  { selection = null as string | null, canApply = true, canStart = true } = {},
) {
  const reply = { session: { id: 's1' } } as unknown as StartReply;
  const d: LaunchDeps = {
    getLaunch: vi.fn(async () => launch),
    runBefore: vi.fn(async () => before),
    profiles: vi.fn(async () => PROFILES),
    remembered: vi.fn(async () => selection),
    plan: vi.fn(
      async () =>
        ({
          selection: 'sel-1',
          selection_name: 'Work',
          config_hash: 'hash-1',
          can_apply: canApply,
        }) as never,
    ),
    bind: vi.fn(async () => ({
      state: canApply ? ('applied' as const) : ('not_applied' as const),
      selection_name: 'Work',
    })),
    preview: vi.fn(
      async () =>
        ({
          plan_token: 'tok',
          provider_version: 'codex-cli 0.159.0',
          can_start: canStart,
        }) as never,
    ),
    start: vi.fn(async () => reply),
    openTerminal: vi.fn(),
    openNative: vi.fn(),
  };
  return { d, reply };
}

describe('openLaunch', () => {
  it('a commands launch opens a terminal with each command typed and Enter after it', async () => {
    const { d } = deps({
      id: 'up',
      name: 'Actualizar sistema',
      directory: '/home/me',
      kind: 'commands',
      commands: ['sudo -v', 'sysupdate'],
    });
    expect(await openLaunch('up', d)).toEqual({ notes: [] });
    expect(d.getLaunch).toHaveBeenCalledWith('up');
    expect(d.openTerminal).toHaveBeenCalledWith(
      'Actualizar sistema',
      '/home/me',
      'sudo -v\rsysupdate\r',
    );
    expect(d.runBefore).not.toHaveBeenCalled();
    expect(d.start).not.toHaveBeenCalled();
  });

  it('a session launch runs before, previews the named profile and starts with that plan', async () => {
    const { d, reply } = deps(session());
    expect(await openLaunch('codex', d)).toEqual({ notes: [] });
    expect(d.runBefore).toHaveBeenCalledWith('codex');
    expect(d.preview).toHaveBeenCalledWith('codex', 'p-full', null);
    expect(d.start).toHaveBeenCalledWith(
      'codex',
      '/home/me',
      'p-full',
      expect.objectContaining({ plan_token: 'tok' }),
      null,
    );
    expect(d.bind).not.toHaveBeenCalled();
    expect(d.openNative).toHaveBeenCalledWith(reply);
    expect(d.openTerminal).not.toHaveBeenCalled();
  });

  it('a before command that fails is said, and the agent still starts', async () => {
    const { d } = deps(session(), [{ command: 'scan', result: 'exit 7' }]);
    const opened = await openLaunch('codex', d);
    expect(opened.notes).toHaveLength(1);
    expect(opened.notes[0]).toContain('exit 7');
    expect(d.start).toHaveBeenCalled();
  });

  it('a profile that does not exist for the agent stops before anything runs', async () => {
    for (const name of ['Missing', 'Claude only']) {
      const { d } = deps(session({ profile: name }));
      await expect(openLaunch('codex', d)).rejects.toThrow(name);
      expect(d.runBefore).not.toHaveBeenCalled();
      expect(d.preview).not.toHaveBeenCalled();
      expect(d.start).not.toHaveBeenCalled();
    }
  });

  it('no profile starts the agent as configured', async () => {
    const { d } = deps(session({ profile: null }));
    await openLaunch('codex', d);
    expect(d.profiles).not.toHaveBeenCalled();
    expect(d.preview).toHaveBeenCalledWith('codex', null, null);
    expect(d.start).toHaveBeenCalledWith(
      'codex',
      '/home/me',
      null,
      expect.anything(),
      null,
    );
  });
});

describe('a session launch and the Arsenal selection its folder remembers', () => {
  const start = {
    selection: 'sel-1',
    config_hash: 'hash-1',
    project: '/home/me',
  };
  it('applies it when it can: the preview and the start carry it, and the terminal is bound', async () => {
    const { d, reply } = deps(session(), undefined, { selection: 'sel-1' });
    const opened = await openLaunch('codex', d);
    expect(d.remembered).toHaveBeenCalledWith('/home/me', 'codex');
    expect(d.plan).toHaveBeenCalledWith('sel-1', 'codex', '/home/me');
    expect(d.preview).toHaveBeenCalledWith('codex', 'p-full', start);
    expect(d.start).toHaveBeenCalledWith(
      'codex',
      '/home/me',
      'p-full',
      expect.anything(),
      start,
    );
    expect(d.bind).toHaveBeenCalledWith(reply.session.id, 'sel-1', 'hash-1');
    expect(opened.notes).toContain('Started with Work applied.');
  });
  it('records it without applying when the plan cannot apply', async () => {
    const { d } = deps(session(), undefined, {
      selection: 'sel-1',
      canApply: false,
    });
    const opened = await openLaunch('codex', d);
    expect(d.preview).toHaveBeenCalledWith('codex', 'p-full', null);
    expect(d.start).toHaveBeenCalledWith(
      'codex',
      '/home/me',
      'p-full',
      expect.anything(),
      null,
    );
    expect(d.bind).toHaveBeenCalled();
    expect(opened.notes.join(' ')).toContain('recorded');
  });
  it('does not start when the applied preview cannot start', async () => {
    const { d } = deps(session(), undefined, {
      selection: 'sel-1',
      canStart: false,
    });
    await expect(openLaunch('codex', d)).rejects.toThrow('Work');
    expect(d.start).not.toHaveBeenCalled();
  });
});

it('full access is shown only for a session whose named profile is bypass for its agent', () => {
  expect(isFullAccess(session(), PROFILES)).toBe(true);
  expect(isFullAccess(session({ profile: 'Strict' }), PROFILES)).toBe(false);
  expect(isFullAccess(session({ profile: null }), PROFILES)).toBe(false);
  expect(
    isFullAccess(
      {
        id: 'x',
        name: 'x',
        directory: '/',
        kind: 'commands',
        commands: ['ls'],
      },
      PROFILES,
    ),
  ).toBe(false);
});
