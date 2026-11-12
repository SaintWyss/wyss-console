import { describe, expect, it, vi } from 'vitest';
import {
  isFullAccess,
  LaunchRefusal,
  openLaunch,
  type LaunchDeps,
  type SelectionName,
} from '../src/lib/features/terminal/launch';
import type { Launch } from '../src/lib/desktop/launch-client';
import type { Profile, StartReply } from '../src/lib/contracts/native';
import { t } from '../src/lib/i18n/index.svelte';

// A note is a sentence and its values; the terminal view draws it through t(), and so do these.
const shown = (note: {
  text: string;
  values?: Record<string, string | number>;
}) => t(note.text, note.values);

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
  preset: null,
  ...over,
});
const selectionOf = (
  id: string,
  name: string,
  provider: 'claude_code' | 'codex',
): SelectionName => ({ id, name, provider });
function deps(
  launch: Launch,
  before = [{ command: 'scan', result: 'ok' }],
  {
    selection = null as string | null,
    canApply = true,
    canStart = true,
    presetId = 'sel-preset' as string | null,
    selections = null as SelectionName[] | null,
  } = {},
) {
  const reply = { session: { id: 's1' } } as unknown as StartReply;
  // By default one Codex selection named Estudio exists, with the id presetId (none when null).
  const named: SelectionName[] =
    selections ?? (presetId ? [selectionOf(presetId, 'Estudio', 'codex')] : []);
  const d: LaunchDeps = {
    getLaunch: vi.fn(async () => launch),
    runBefore: vi.fn(async () => before),
    profiles: vi.fn(async () => PROFILES),
    remembered: vi.fn(async () => selection),
    selections: vi.fn(async () => named),
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
    expect(shown(opened.notes[0])).toContain('exit 7');
    expect(d.start).toHaveBeenCalled();
  });

  it('the preset a launch names wins over the one its folder remembers', async () => {
    const { d } = deps(session({ preset: 'Estudio' }), undefined, {
      selection: 'sel-remembered',
    });
    await openLaunch('codex', d);
    expect(d.selections).toHaveBeenCalled();
    expect(d.remembered).not.toHaveBeenCalled();
    expect(d.plan).toHaveBeenCalledWith('sel-preset', 'codex', '/home/me');
  });

  it('a launch with no preset keeps the one its folder remembers', async () => {
    const { d } = deps(session(), undefined, { selection: 'sel-remembered' });
    await openLaunch('codex', d);
    expect(d.selections).not.toHaveBeenCalled();
    expect(d.plan).toHaveBeenCalledWith('sel-remembered', 'codex', '/home/me');
  });

  it('a preset with no name for this agent stops the launch before anything runs, and says which', async () => {
    const { d } = deps(session({ preset: 'Missing' }), undefined, {
      presetId: null,
    });
    const refusal = await openLaunch('codex', d).catch(
      (error: unknown) => error,
    );
    expect(refusal).toBeInstanceOf(LaunchRefusal);
    expect((refusal as Error).message).toContain('Missing');
    // The sentence and its values reach the view, which draws them through t().
    const { template, values } = refusal as LaunchRefusal;
    expect(template).toContain('{name}');
    expect(values).toEqual({ name: 'Missing' });
    expect(d.runBefore).not.toHaveBeenCalled();
    expect(d.plan).not.toHaveBeenCalled();
    expect(d.start).not.toHaveBeenCalled();
  });

  it('two selections of this agent with the preset name stop the launch before anything runs, and say to delete the extra ones', async () => {
    const { d } = deps(session({ preset: 'Estudio' }), undefined, {
      selections: [
        selectionOf('a', 'Estudio', 'codex'),
        selectionOf('b', 'Estudio', 'codex'),
      ],
    });
    const refusal = await openLaunch('codex', d).catch(
      (error: unknown) => error,
    );
    expect(refusal).toBeInstanceOf(LaunchRefusal);
    // Its own sentence, not the "no preset" one: the owner has one too many, not none.
    const { template, values } = refusal as LaunchRefusal;
    expect(template).toContain('More than one preset');
    expect(template).not.toContain('No preset named');
    expect(values).toEqual({ name: 'Estudio' });
    expect(d.runBefore).not.toHaveBeenCalled();
    expect(d.plan).not.toHaveBeenCalled();
    expect(d.start).not.toHaveBeenCalled();
  });

  it('a same-named preset of the other agent is not a second match', async () => {
    const { d } = deps(session({ preset: 'Estudio' }), undefined, {
      selections: [
        selectionOf('a', 'Estudio', 'claude_code'),
        selectionOf('b', 'Estudio', 'codex'),
      ],
    });
    await openLaunch('codex', d);
    expect(d.plan).toHaveBeenCalledWith('b', 'codex', '/home/me');
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
    expect(opened.notes.map(shown)).toContain('Started with Work applied.');
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
    expect(opened.notes.map(shown).join(' ')).toContain('recorded');
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

it('full access is shown for a profile that inherits it, and a broken chain gives nothing', () => {
  const child = (id: string, name: string, parent: string | null) => {
    const p = profile(id, name, null);
    (p.body as { extends: string | null }).extends = parent;
    return p;
  };
  const list = [
    ...PROFILES,
    child('p-kid', 'Kid', 'p-full'),
    child('p-grandkid', 'Grandkid', 'p-kid'),
    child('p-orphan', 'Orphan', 'p-gone'),
    child('p-a', 'Loop A', 'p-b'),
    child('p-b', 'Loop B', 'p-a'),
  ];
  expect(isFullAccess(session({ profile: 'Kid' }), list)).toBe(true);
  expect(isFullAccess(session({ profile: 'Grandkid' }), list)).toBe(true);
  expect(isFullAccess(session({ profile: 'Orphan' }), list)).toBe(false);
  expect(isFullAccess(session({ profile: 'Loop A' }), list)).toBe(false);
  // A nearer profile that sets another word wins over an inherited bypass.
  const safer = profile('p-safer', 'Safer', 'ask');
  (safer.body as { extends: string | null }).extends = 'p-full';
  expect(isFullAccess(session({ profile: 'Safer' }), [...list, safer])).toBe(
    false,
  );
});
