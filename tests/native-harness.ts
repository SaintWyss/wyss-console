import { vi } from 'vitest';
import type { NativeClient } from '../src/lib/features/native/controller';
import {
  parseLauncher,
  parseProfiles,
  parseSession,
  parseStartReply,
  type Launcher,
  type NativeSession,
  type Profile,
  type StartReply,
} from '../src/lib/contracts/native';

export const TERMINAL = '66666666-6666-4666-8666-666666666666';

export function launcher(over: Record<string, unknown> = {}): Launcher {
  return parseLauncher({
    agents: [
      {
        agent_id: 'claude_code',
        name: 'Claude Code',
        executable: 'claude',
        installed: true,
        path: '/home/u/.local/bin/claude',
        version: '2.1.303 (Claude Code)',
      },
      {
        agent_id: 'codex',
        name: 'Codex',
        executable: 'codex',
        installed: false,
        path: null,
        version: null,
      },
    ],
    shell: { path: '/usr/bin/fish', family: 'fish', through_shell: true },
    projects: [
      {
        path: '/home/u',
        home: true,
        default_agent: null,
        default_profile: null,
        last_used: 0,
      },
      {
        path: '/home/u/work/app',
        home: false,
        default_agent: 'claude_code',
        default_profile: 'template-review',
        last_used: 5,
      },
    ],
    ...over,
  });
}

export function session(over: Record<string, unknown> = {}): NativeSession {
  return parseSession({
    id: 'a'.repeat(32),
    agent_id: 'claude_code',
    agent_name: 'Claude Code',
    version: '2.1.303 (Claude Code)',
    project: '/home/u',
    profile: null,
    through_shell: true,
    state: 'running',
    started_at: 1_700_000_000,
    ended_at: null,
    exit_code: null,
    exit_signal: null,
    error: null,
    restart_of: null,
    restarts: 0,
    plane: 'native',
    terminal_id: TERMINAL,
    pid: 4242,
    usage: null,
    ...over,
  });
}

export function reply(over: Record<string, unknown> = {}): StartReply {
  const s = session(over);
  return parseStartReply({
    session: s,
    terminal: {
      terminal_id: s.terminal_id,
      shell: '/usr/bin/fish',
      cwd: s.project,
      observed_pid: 4242,
      size: { rows: 24, cols: 80 },
      state: 'running',
      exit: null,
      error: null,
    },
  });
}

const body = (name: string, over: Record<string, unknown> = {}) => ({
  name,
  applies_to: ['claude_code', 'codex'],
  extends: null,
  model: null,
  permission: null,
  hooks: null,
  skills_off: [],
  notes: '',
  ...over,
});

export function profiles(): Profile[] {
  return parseProfiles([
    {
      id: 'template-review',
      template: true,
      current: 0,
      versions: [],
      body: body('Review, read only', { permission: 'read_only' }),
      from: {},
      refers_to_missing: false,
    },
    {
      id: 'template-exploration',
      template: true,
      current: 0,
      versions: [],
      body: body('Exploration, fewer hooks', {
        applies_to: ['claude_code'],
        hooks: 'off',
      }),
      from: {},
      refers_to_missing: false,
    },
    {
      id: 'b'.repeat(32),
      template: false,
      current: 2,
      versions: [
        { n: 1, saved_at: 1_700_000_000, hash: 'aa', name: 'Mine' },
        { n: 2, saved_at: 1_700_000_100, hash: 'bb', name: 'Mine' },
      ],
      body: body('Mine', { model: 'sonnet' }),
      from: {},
      refers_to_missing: false,
    },
  ]);
}

/** A client over canned answers; every call is a spy the test can read. */
export function fakeClient(over: Partial<NativeClient> = {}) {
  const client: NativeClient = {
    launcher: vi.fn(async () => launcher()),
    sessions: vi.fn(async () => [] as NativeSession[]),
    profiles: vi.fn(async () => profiles()),
    preview: vi.fn(async () => ({
      applied: [],
      with_cost: [],
      not_applicable: [],
      can_start: true,
      through_shell: true,
      warnings: [],
      plan_token: 'preview-token',
      provider_version: '2.1.309 (Claude Code)',
    })),
    start: vi.fn(async () => reply()),
    prepareHandoff: vi.fn(async () => ({
      text: '# Handoff from Claude Code\n\nbody\n',
      agent_id: 'claude_code' as const,
      project: '/home/u',
      screen_included: true,
    })),
    saveHandoff: vi.fn(async () => true),
    restart: vi.fn(async () =>
      reply({ id: 'c'.repeat(32), restart_of: 'a'.repeat(32), restarts: 1 }),
    ),
    stop: vi.fn(async () => undefined),
    forget: vi.fn(async () => undefined),
    chooseProject: vi.fn(async () => null),
    setDefault: vi.fn(async () => undefined),
    createProfile: vi.fn(async () => 'd'.repeat(32)),
    saveProfile: vi.fn(async () => undefined),
    deleteProfile: vi.fn(async () => undefined),
    duplicateProfile: vi.fn(async () => 'e'.repeat(32)),
    restoreProfile: vi.fn(async () => undefined),
    exportProfile: vi.fn(async () => true),
    importProfile: vi.fn(async () => null),
    ...over,
  };
  return client;
}
