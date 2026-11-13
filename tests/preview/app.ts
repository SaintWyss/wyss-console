/**
 * A browser preview of the whole window, for looking at it: the application with a scripted
 * desktop behind it. Nothing here is shipped: the build bundles `index.html` only.
 *
 *   npm run dev, then open /tests/preview/app.html?v=arsenal&theme=light
 *
 * `v`: the view the window opens on (console, workspace, arsenal, sessions, settings, …).
 * `theme`: dark (default), light or high_contrast. `state`: ready (default), empty or failed.
 * `glass`: native or transparent, to see the window as KWin composes it.
 */
import { mockIPC } from '@tauri-apps/api/mocks';
import { mount } from 'svelte';
import App from '../../src/App.svelte';
import '../../src/app.css';
import { DEFAULT_PREFERENCES } from '../../src/lib/contracts/settings';
import {
  NAV_ITEMS,
  PROJECTS_NAV_ITEM,
  PROVIDERS_NAV_ITEM,
  RUNTIMES_NAV_ITEM,
} from '../../src/lib/shell/nav';
import approvals from '../fixtures/approval-list-v1.json';
import inventory from '../fixtures/configuration-inventory-v1.json';
import views from '../fixtures/authority-view-v1.json';
import pages from '../fixtures/session-page-v4.json';
import catalogue from '../fixtures/model-catalogue-v1.json';

type Json = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const query = new URLSearchParams(location.search);
const view = query.get('v') ?? 'console';
const theme = query.get('theme') ?? 'dark';
const state = query.get('state') ?? 'ready';
const glass = query.get('glass');
if (glass) {
  document.documentElement.dataset.glass = glass;
  // What KWin shows through a transparent window: the wallpaper and the windows behind it.
  const desktop = document.createElement('style');
  desktop.textContent = `html::before { content: ''; position: fixed; inset: 0; z-index: -2;
    background: repeating-linear-gradient(135deg, #3b6db3 0 40px, #e9c46a 40px 80px, #f4f4f4 80px 120px, #1d3557 120px 160px); }`;
  document.head.append(desktop);
}
const empty = state === 'empty';
const failed = state === 'failed';

const never = () => new Promise(() => undefined);
const now = Math.floor(Date.now() / 1000);
const terminal = {
  terminal_id: '55555555-5555-4555-8555-555555555555',
  shell: '/usr/bin/fish',
  cwd: '/home/u/Github_repos/wyss-console',
  observed_pid: 4242,
  size: { rows: 24, cols: 80 },
  state: 'running',
  exit: null,
  error: null,
};

const skill = (name: string, description: string, over: Json = {}): Json => ({
  id: `cap-${name}`,
  provider: 'claude_code',
  source: 'claude_user_skills',
  kind: 'skill',
  name,
  description,
  origin: `~/.claude/skills/${name}`,
  revision: 1,
  revisions: 1,
  tree_hash: 'c'.repeat(64),
  updated_at: now - 3600,
  compatibility: { status: 'supported', reason: 'own format' },
  ...over,
});
const library = [
  skill(
    'gsap-react',
    'Official GSAP skill for React: useGSAP, refs, cleanup on unmount.',
  ),
  skill(
    'frontend-design',
    'Distinctive, intentional visual design for new or reshaped interfaces.',
    { revision: 3, revisions: 3 },
  ),
  skill(
    'systematic-debugging',
    'Root cause of an observed failure when it is intermittent or crosses components.',
  ),
  skill(
    'ui-ux-pro-max',
    'UI/UX design intelligence: styles, palettes, font pairings and UX guidelines.',
  ),
  skill(
    'gsd-execute-phase',
    'Execute all plans in a phase with dependency-aware wave parallelization.',
    {
      provider: 'codex',
      source: 'codex_user_skills',
      origin: '~/.codex/skills/gsd-execute-phase',
      compatibility: {
        status: 'unverified',
        reason: 'not qualified for this Codex build',
      },
    },
  ),
  skill(
    'markdown-converter',
    'Converts PDF, Office, HTML and audio to Markdown.',
    {
      compatibility: {
        status: 'unsupported',
        reason: 'needs a binary that is not installed',
      },
    },
  ),
];

const delegations = [
  {
    id: 'a'.repeat(32),
    parent_session: '1234567890abcdef',
    parent_provider: 'claude_code',
    provider: 'codex',
    model: 'gpt-6.1-sol',
    effort: 'medium',
    title: 'Write the fixture loader for the preview harness',
    preset: 'Frontend preset',
    selection: 'Worker selection',
    workspace: '/home/u/.local/state/wyss-console/delegations/a/workspace',
    created_at: now - 240,
    ended_at: null,
    timeout_minutes: 20,
    status: 'running',
    error: null,
    result: null,
    steps: 3,
    verification: { state: 'pending', problems: [], model: null },
    applied: {
      skills: [{ name: 'gsap-react', revision: 1 }],
      switched_off: 179,
    },
    artifacts: [],
  },
  {
    id: 'b'.repeat(32),
    parent_session: '1234567890abcdef',
    parent_provider: 'codex',
    provider: 'claude_code',
    model: 'haiku',
    effort: 'low',
    title: 'Summarize the failing test output',
    preset: null,
    selection: null,
    workspace: '/home/u/.local/state/wyss-console/delegations/b/workspace',
    created_at: now - 900,
    ended_at: now - 700,
    timeout_minutes: 20,
    status: 'succeeded',
    error: null,
    result:
      'Three failures share one cause: the fixture clock starts before the request.',
    steps: 5,
    verification: {
      state: 'verified',
      problems: [],
      model: 'claude-haiku-5-5',
    },
    applied: { skills: [], switched_off: 0 },
    artifacts: [{ path: 'summary.md', size: 1840 }],
  },
  {
    id: 'c'.repeat(32),
    parent_session: 'fedcba0987654321',
    parent_provider: 'claude_code',
    provider: 'codex',
    model: 'gpt-6-luna',
    effort: 'high',
    title: 'Port the readiness poller to the new lock',
    preset: null,
    selection: 'Worker selection',
    workspace: '/home/u/.local/state/wyss-console/delegations/c/workspace',
    created_at: now - 1800,
    ended_at: now - 1500,
    timeout_minutes: 20,
    status: 'failed',
    error: 'provider_exited',
    result: null,
    steps: 2,
    verification: {
      state: 'mismatch',
      problems: ['model differs from the request'],
      model: 'gpt-6-luna',
    },
    applied: {
      skills: [{ name: 'systematic-debugging', revision: 1 }],
      switched_off: 180,
    },
    artifacts: [],
  },
];

const launcher = {
  agents: [
    {
      agent_id: 'claude_code',
      name: 'Claude Code',
      executable: 'claude',
      installed: true,
      path: '/home/u/.local/bin/claude',
      version: '2.1.316 (Claude Code)',
    },
    {
      agent_id: 'codex',
      name: 'Codex',
      executable: 'codex',
      installed: true,
      path: '/home/u/.local/bin/codex',
      version: 'codex-cli 0.159.0',
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
      path: '/home/u/Github_repos/wyss-console',
      home: false,
      default_agent: 'claude_code',
      default_profile: null,
      last_used: now - 60,
    },
  ],
  m9_native_status: 'ready',
};

const body = (name: string, over: Json = {}) => ({
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
const profiles = [
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
    id: 'b'.repeat(32),
    template: false,
    current: 2,
    versions: [
      { n: 1, saved_at: now - 9000, hash: 'aa', name: 'Mine' },
      { n: 2, saved_at: now - 100, hash: 'bb', name: 'Mine' },
    ],
    body: body('Daily driver', { model: 'sonnet' }),
    from: {},
    refers_to_missing: false,
  },
];
const nativeSessions = [
  {
    id: 'd'.repeat(32),
    agent_id: 'claude_code',
    agent_name: 'Claude Code',
    version: '2.1.316 (Claude Code)',
    project: '/home/u/Github_repos/wyss-console',
    profile: null,
    through_shell: true,
    state: 'running',
    started_at: now - 1200,
    ended_at: null,
    exit_code: null,
    exit_signal: null,
    error: null,
    restart_of: null,
    restarts: 0,
    plane: 'native',
    terminal_id: terminal.terminal_id,
    pid: 4242,
    usage: { cpu_percent: 3.2, rss_bytes: 412_000_000, processes: 6 },
  },
];

const items = (names: string[]) =>
  names.map((n) => ({ capability: `cap-${n}`, revision: null }));
const presets = [
  {
    id: 'p1',
    name: 'Frontend preset',
    description: 'Design and motion skills for UI work.',
    items: items(['gsap-react', 'frontend-design', 'ui-ux-pro-max']),
    created_at: now - 86400,
    updated_at: now - 3600,
  },
];
const selections = [
  {
    id: 's1',
    name: 'Worker selection',
    provider: 'claude_code',
    items: items(['gsap-react']),
    composites: [],
    created_at: now - 86400,
    updated_at: now - 3600,
  },
];

const settings = {
  schema_version: 1,
  storage: 'ready',
  preferences: {
    ...DEFAULT_PREFERENCES,
    interface: { landing_page: 'console', theme, language: 'en' },
  },
  about: { version: '0.2.0', revision: '426ddb7', platform: 'linux-x86_64' },
};

const connected = {
  ...views.connected,
  agent: { ...views.connected.agent, actual: 'ready', model_count: 7 },
};

function fail(code = 'unavailable') {
  throw { code, retry: true, retry_in_ms: 3000 };
}

mockIPC((command, payload) => {
  const request = (payload as { request?: Json } | undefined)?.request ?? {};
  switch (command) {
    case 'get_app_info':
      return { schema_version: 1, name: 'Wyss Console', version: '0.2.0' };
    case 'settings_read':
      return settings;
    case 'authority_state':
      return request.after === null ? connected : never();
    case 'authority_lifecycle':
      return never();
    case 'f8_state':
      return { schema_version: 1, gate: 'inactive' };
    case 'configuration_inventory':
      if (failed) fail();
      return inventory;
    case 'configuration_changes':
      return { result: 'same', revision: 1 };
    case 'managed_sessions_page':
      return empty ? (pages.empty ?? pages.loaded) : pages.loaded;
    case 'managed_turns':
      return { schema_version: 1, turns: [], next: null };
    case 'managed_approvals':
      return approvals.empty;
    case 'managed_model_catalogue':
      return catalogue;
    case 'credentials_list':
      return { schema_version: 1, credentials: [] };
    case 'arsenal_capabilities':
      if (failed) fail();
      return empty ? [] : library;
    case 'arsenal_capability': {
      const summary =
        library.find((item) => item.id === request.id) ?? library[0]!;
      return {
        summary,
        revision: summary.revision,
        revisions: [
          {
            number: summary.revision,
            tree_hash: summary.tree_hash,
            origin: summary.origin,
            created_at: now - 3600,
            files: 3,
            bytes: 18_240,
            current: true,
          },
        ],
        files: [
          { path: 'SKILL.md', sha256: 'a'.repeat(64), size: 12_400 },
          {
            path: 'references/patterns.md',
            sha256: 'b'.repeat(64),
            size: 5_100,
          },
          { path: 'scripts/check.sh', sha256: 'c'.repeat(64), size: 740 },
        ],
        compatibility: summary.compatibility,
        dependencies: [
          { kind: 'tool', name: 'node', state: 'present' },
          { kind: 'mcp_server', name: 'context7', state: 'missing' },
        ],
      };
    }
    case 'arsenal_presets':
      return empty ? [] : presets;
    case 'arsenal_selections':
      return empty ? [] : selections;
    case 'arsenal_terminal_bindings':
      return [];
    // Preview answers for the launch editor: example launches and sizes, not anyone's files.
    case 'arsenal_roles':
    case 'arsenal_role_write':
      return {
        custom: [],
        assigned: { 'wyss-frontend': 'worker', estudiar: 'researcher' },
      };
    case 'arsenal_environment_options':
      return {
        hooks: [
          {
            key: 'k1',
            event: 'SessionStart',
            matcher: '',
            command: 'auto-orient.sh',
          },
          {
            key: 'k2',
            event: 'PostToolUse',
            matcher: 'Edit',
            command: 'format.sh',
          },
        ],
        mcp: ['context7', 'playwright', 'notebooklm'],
        plugins: ['caveman@caveman'],
        rules: ['invariantes.md', 'ux.md'],
        account_skills: ['anthropic-skills:pdf'],
        instructions: true,
        token_stored: true,
        sizes: {
          instructions: 26000,
          'rule:invariantes.md': 22000,
          'rule:ux.md': 6000,
          'account:anthropic-skills:pdf': 300,
          'plugin:caveman@caveman': 4000,
        },
      };
    case 'arsenal_environment':
      return {
        instructions: true,
        rules_off: [],
        hooks_off: [],
        mcp: null,
        plugins_off: [],
        memory: true,
        agents: true,
        account_skills_off: [],
      };
    case 'arsenal_project_selection':
      return null;
    case 'launch_list':
      return {
        folder: '/home/owner/.config/wyss-console/launch',
        launches: [
          {
            id: 'estudiar',
            name: 'Estudiar',
            directory: '/home/owner/UTN',
            kind: 'session',
            agent: 'claude_code',
            profile: null,
            before: [],
            preset: null,
          },
          {
            id: 'wyss-frontend',
            name: 'wyss frontend',
            directory: '/home/owner/Github_repos/example',
            kind: 'session',
            agent: 'claude_code',
            profile: null,
            before: [],
            preset: null,
          },
          {
            id: 'revision',
            name: 'Revisión con Codex',
            directory: '/home/owner/Github_repos',
            kind: 'session',
            agent: 'codex',
            profile: null,
            before: [],
            preset: null,
          },
        ],
        refused: [],
      };
    case 'arsenal_sync_status':
    case 'arsenal_sync_now':
      return {
        enabled: true,
        folder: '~/.local/share/wyss-console-sync/arsenal',
        device: 'a'.repeat(32),
        host: 'saint-acer',
        generation: 3,
        last_run: now - 20,
        last_success: now - 20,
        error: null,
        published: 412,
        pending: [],
        conflicts: [],
        rejected: [],
        removed: [],
        peers: [
          {
            device: 'b'.repeat(32),
            host: 'Santiago-PC',
            last_heard: now - 95,
            applied_mine: 412,
            published_mine: 412,
          },
        ],
      };
    case 'native_launcher':
      return launcher;
    case 'native_readiness':
      return { m9_native_status: 'ready' };
    case 'native_profiles':
      return profiles;
    case 'native_sessions':
      return empty ? [] : nativeSessions;
    case 'native_delegations':
      if (failed) fail();
      return empty ? [] : delegations;
    case 'host_terminals':
      return [];
    case 'host_info':
      return { schema_version: 1, state: 'unavailable' };
    case 'layout_read':
      return null;
    case 'terminal_read_output':
      return never();
    // A preview answer, for looking at the session bar: not what any folder holds.
    case 'terminal_git_status':
      return {
        kind: 'repo',
        dir: '/home/owner/Github_repos/example',
        home: '/home/owner',
        branch: 'main',
        changes: 3,
        ahead: 2,
        truncated: false,
      };
    case 'm3_state':
      return { schema_version: 1, state: 'inactive' };
    default:
      return terminal;
  }
});

class Resize {
  observe() {}
  disconnect() {}
}
(globalThis as Json).ResizeObserver ??= Resize;

const target = document.getElementById('app');
if (!target) throw new Error('Application root is missing');
mount(App, { target });

// Opens the view as a person does: by its item in the sidebar.
const label =
  [...NAV_ITEMS, RUNTIMES_NAV_ITEM, PROVIDERS_NAV_ITEM, PROJECTS_NAV_ITEM].find(
    (item) => item.id === view,
  )?.label ?? { activity: 'Activity', launches: 'Workspace' }[view];
setTimeout(() => {
  [...document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item')]
    .find((b) => b.textContent?.trim() === label)
    ?.click();
}, 300);
