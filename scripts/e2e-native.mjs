// Native end-to-end harness: a disposable sandbox, a private `systemd --user` manager, a
// nested KWin display and the packaged Wyss Console, plus the seeder that fills the
// authority through its own protocol. Every process `up` starts is recorded in the
// sandbox's state file and stopped by recorded PID or process group. Nothing is found by
// name, and nothing outside the sandbox is written, read or stopped.
//
//   node scripts/e2e-native.mjs up                 start the stack; prints the sandbox root
//   node scripts/e2e-native.mjs seed <root>        two sessions in one workspace, 45 turns each
//   node scripts/e2e-native.mjs down <root>        stop what `up` started, remove the sandbox
//
// Environment: WYSS_E2E_PACKAGE (default `.artifacts/package/<name>`), WYSS_E2E_LOGS
// (directory that keeps the logs after `down`), WYSS_E2E_TURNS (default 45).
import { spawn, spawnSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import {
  assertSandboxRoot,
  installFixtureCodex,
  sandboxEnv,
} from './e2e-guard.mjs';

const REPO = resolve('.');
const FIXTURE = join(REPO, 'src-tauri/tests/fixtures/codex_runtime.py');
const DISPLAY = ':77';
const SCENARIO = 'echo';
const KWIN_SOCKET = 'wl-accept';

/** @typedef {{ root: string, pids: Record<string, number>, groups: number[], workspace: string }} State */

/** @param {string} root */
function statePath(root) {
  return join(root, 'state.json');
}

/** @param {string} root @returns {State} */
function readState(root) {
  return JSON.parse(readFileSync(statePath(root), 'utf8'));
}

/** @param {string} root @param {State} state */
function writeState(root, state) {
  writeFileSync(statePath(root), `${JSON.stringify(state, null, 2)}\n`);
}

/** @param {string} path @param {number} seconds @returns {Promise<boolean>} */
async function waitFor(path, seconds) {
  for (let i = 0; i < seconds * 10; i++) {
    if (existsSync(path)) return true;
    await new Promise((done) => setTimeout(done, 100));
  }
  return false;
}

/**
 * Starts a child in its own process group, output to a log under the sandbox.
 * @param {string} root
 * @param {string} name
 * @param {string} command
 * @param {string[]} args
 * @param {Record<string, string>} env
 * @returns {number} the process group id, which is the child's pid
 */
function launch(root, name, command, args, env) {
  const logs = join(root, 'logs');
  mkdirSync(logs, { recursive: true });
  const out = join(logs, `${name}.log`);
  const log = openSync(out, 'a');
  const child = spawn(command, args, {
    env,
    detached: true,
    stdio: ['ignore', log, log],
  });
  child.unref();
  if (child.pid === undefined) throw new Error(`could not start ${name}`);
  return child.pid;
}

/**
 * The Plasma session variables a KWin compositor and the desktop expect. They name the
 * session type and desktop only; they carry no credential and no path of the real session.
 * @returns {Record<string, string>}
 */
function sessionVars() {
  return {
    KDE_FULL_SESSION: 'true',
    KDE_SESSION_VERSION: '6',
    XDG_CURRENT_DESKTOP: 'KDE',
    XDG_SESSION_TYPE: 'wayland',
    DESKTOP_SESSION: 'plasma',
  };
}

/** @param {string} root @returns {Record<string, string>} */
function envOf(root) {
  return sandboxEnv({
    root,
    fixtureDir: root,
    scenario: SCENARIO,
    display: '',
  });
}

/** @param {string} root @param {string[]} args */
function sctl(root, args) {
  return spawnSync('systemctl', ['--user', ...args], {
    env: envOf(root),
    encoding: 'utf8',
  });
}

/**
 * Whether a process (or group leader) with this pid still exists.
 * @param {number} pid
 * @returns {boolean}
 */
function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/**
 * Asks each recorded group to stop, then kills what did not stop. Returns the groups that
 * survived even SIGKILL, which the caller must report as a failure.
 * @param {number[]} groups
 * @returns {Promise<number[]>}
 */
async function stopGroups(groups) {
  for (const group of groups) {
    try {
      process.kill(-group, 'SIGTERM');
    } catch {
      // Already gone.
    }
  }
  await new Promise((done) => setTimeout(done, 3000));
  for (const group of groups) {
    if (alive(group)) {
      try {
        process.kill(-group, 'SIGKILL');
      } catch {
        // Gone between the check and the kill.
      }
    }
  }
  await new Promise((done) => setTimeout(done, 1000));
  return groups.filter((group) => alive(group));
}

/** @param {string} pkg @returns {string} */
function productBinary(pkg) {
  return join(pkg, 'bin', 'wyss-console');
}

/** @returns {string} the packaged directory the harness runs */
function packageDir() {
  const named = process.env.WYSS_E2E_PACKAGE;
  if (named) return resolve(named);
  const base = join(REPO, '.artifacts/package');
  const [only] = readdirSync(base).filter((entry) =>
    entry.startsWith('wyss-console-'),
  );
  if (!only)
    throw new Error(
      'no packaged build under .artifacts/package; run desktop:package',
    );
  return join(base, only);
}

async function up() {
  const lock = `/tmp/.X${DISPLAY.slice(1)}-lock`;
  if (existsSync(lock)) {
    throw new Error(
      `display ${DISPLAY} is already in use (${lock}); refusing to share it`,
    );
  }
  const root = mkdtempSync('/tmp/wacc-');
  chmodSync(root, 0o700);
  for (const dir of [
    'run',
    'home',
    'cfg',
    'data',
    'state',
    'cache',
    'bin',
    'codex-home',
    'workspace',
  ]) {
    mkdirSync(join(root, dir), { recursive: true, mode: 0o700 });
  }
  assertSandboxRoot(root, { home: homedir(), uid: process.getuid?.() ?? -1 });
  writeFileSync(
    join(root, 'codex-home', 'models_cache.json'),
    '{"fetched_at":"2026-10-07T00:00:00Z","models":[{"slug":"fixture-model","tool_mode":"code_mode_only","shell_type":"unified_exec","apply_patch_tool_type":"freeform","multi_agent_version":"v2","experimental_supported_tools":["send_user_message_async","clock"]}]}',
  );
  installFixtureCodex(root, FIXTURE);
  const pkg = packageDir();
  /** @type {State} */
  const state = {
    root,
    pids: {},
    groups: [],
    workspace: join(root, 'workspace'),
  };
  writeState(root, state);

  // 1. Private user service manager: the authority runs under it, with the fixture PATH.
  state.pids.systemd = launch(
    root,
    'systemd',
    '/usr/lib/systemd/systemd',
    ['--user'],
    envOf(root),
  );
  writeState(root, state);
  if (!(await waitFor(join(root, 'run', 'systemd', 'private'), 15))) {
    throw new Error('private systemd --user did not start');
  }
  // The manager replaces the PATH it was started with. The authority must find the
  // fixture first on the sandbox PATH and nothing else, so the PATH is set on the manager
  // and read back: a mismatch stops the start.
  const path = `${join(root, 'bin')}:/usr/bin:/bin`;
  const set = sctl(root, ['set-environment', `PATH=${path}`]);
  const shown = sctl(root, ['show-environment']).stdout;
  if (set.status !== 0 || !shown.split('\n').includes(`PATH=${path}`)) {
    throw new Error(
      'the manager PATH is not the sandbox PATH; refusing to start the authority',
    );
  }

  // 2. Nested display, bound to the sandbox runtime directory.
  const runEnv = {
    ...envOf(root),
    ...sessionVars(),
    XDG_RUNTIME_DIR: join(root, 'run'),
  };
  const kwin = launch(
    root,
    'kwin-wayland',
    'dbus-run-session',
    [
      '--',
      'kwin_wayland',
      '--virtual',
      '--width',
      '1400',
      '--height',
      '900',
      '--socket',
      KWIN_SOCKET,
      '--no-lockscreen',
      '--no-global-shortcuts',
    ],
    runEnv,
  );
  state.groups.push(kwin);
  if (!(await waitFor(join(root, 'run', KWIN_SOCKET), 30)))
    throw new Error('nested compositor did not start');
  const xwayland = launch(
    root,
    'xwayland',
    'Xwayland',
    [DISPLAY, '-geometry', '1400x900', '-shm', '-nolisten', 'tcp', '-ac'],
    { ...runEnv, WAYLAND_DISPLAY: KWIN_SOCKET },
  );
  state.groups.push(xwayland);
  state.pids.xwayland = xwayland;
  await new Promise((done) => setTimeout(done, 3000));

  // 3. The packaged desktop, in the same sandbox.
  launchApp(root, state, pkg);
  writeState(root, state);
  console.log(root);
}

/**
 * Starts the packaged desktop in the sandbox. Used at start and by relaunch-app.
 * @param {string} root
 * @param {State} state
 * @param {string} pkg
 */
function launchApp(root, state, pkg) {
  /** @type {Record<string, string>} */
  const appEnv = {
    ...envOf(root),
    XDG_RUNTIME_DIR: join(root, 'run'),
    DISPLAY,
    GDK_BACKEND: 'x11',
    XDG_CURRENT_DESKTOP: 'KDE',
    KDE_SESSION_VERSION: '6',
    NO_AT_BRIDGE: '1',
  };
  state.pids.app = launch(
    root,
    'app',
    'dbus-run-session',
    ['--', productBinary(pkg)],
    appEnv,
  );
  state.groups.push(state.pids.app);
}

/**
 * Stops only the desktop and starts it again in the same sandbox. The authority keeps
 * running: this is an application restart, not an authority restart.
 * @param {string} root
 */
async function relaunchApp(root) {
  const state = readState(root);
  const previous = state.pids.app;
  const survivors = await stopGroups([previous]);
  if (survivors.length > 0) {
    throw new Error(`the desktop survived SIGKILL: ${survivors.join(', ')}`);
  }
  state.groups = state.groups.filter((group) => group !== previous);
  launchApp(root, state, packageDir());
  writeState(root, state);
  console.log(`app restarted: ${previous} -> ${state.pids.app}`);
}

/** @param {string} root */
function seed(root) {
  const state = readState(root);
  const workspace = state.workspace;
  const report = join(root, 'seed-report.json');
  const build = spawnSync(
    process.execPath,
    [
      'scripts/tool.mjs',
      'cargo',
      'test',
      '--locked',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      '--test',
      'e2e_seed',
      '--no-run',
    ],
    { cwd: REPO, encoding: 'utf8' },
  );
  if (build.status !== 0)
    throw new Error(`seeder build failed: ${build.stderr}`);
  const binary = findSeeder();
  const result = spawnSync(
    binary,
    [
      '--ignored',
      '--exact',
      'seed_two_sessions_in_one_workspace',
      '--nocapture',
    ],
    {
      env: {
        ...envOf(root),
        WYSS_E2E_SANDBOX: root,
        WYSS_E2E_WORKSPACE: workspace,
        WYSS_E2E_REPORT: report,
        WYSS_E2E_TURNS: process.env.WYSS_E2E_TURNS ?? '45',
      },
      encoding: 'utf8',
      timeout: 600000,
    },
  );
  process.stdout.write(result.stdout ?? '');
  process.stderr.write(result.stderr ?? '');
  if (result.status !== 0)
    throw new Error(`seeder failed with status ${result.status}`);
  console.log(readFileSync(report, 'utf8'));
}

/** @returns {string} the built seeder test binary */
function findSeeder() {
  const deps = join(REPO, 'src-tauri/target/debug/deps');
  const [found] = readdirSync(deps).filter((name) =>
    /^e2e_seed-[0-9a-f]+$/.test(name),
  );
  if (!found) throw new Error('seeder binary not built');
  return join(deps, found);
}

/** @param {string} root */
async function down(root) {
  const state = readState(root);
  sctl(root, ['stop', 'wyss-control.service']);
  const groups = [
    ...state.groups,
    ...(state.pids.systemd ? [state.pids.systemd] : []),
  ];
  const survivors = await stopGroups(groups);
  if (survivors.length > 0) {
    throw new Error(
      `processes survived SIGKILL, sandbox kept for inspection: ${survivors.join(', ')}`,
    );
  }
  // The display lock and socket are not in the sandbox. They are removed only when they
  // name the Xwayland this run started, and that process is gone.
  const lock = `/tmp/.X${DISPLAY.slice(1)}-lock`;
  if (existsSync(lock) && state.pids.xwayland !== undefined) {
    const owner = Number(readFileSync(lock, 'utf8').trim());
    if (owner === state.pids.xwayland && !alive(owner)) {
      rmSync(lock, { force: true });
      rmSync(`/tmp/.X11-unix/X${DISPLAY.slice(1)}`, { force: true });
    }
  }
  const keep = process.env.WYSS_E2E_LOGS;
  if (keep) {
    mkdirSync(keep, { recursive: true });
    for (const log of readdirSync(join(root, 'logs')))
      copyFileSync(join(root, 'logs', log), join(keep, log));
  }
  // The systemd manager makes its own directories read-only. They are ours: restore the
  // owner's write bit inside the sandbox only, then remove the sandbox.
  spawnSync('chmod', ['-R', 'u+rwx', root]);
  rmSync(root, { recursive: true, force: true });
  console.log(`removed ${root}`);
}

const [command, root] = process.argv.slice(2);
if (command === 'up') {
  await up();
} else if (command === 'seed' && root) {
  seed(root);
} else if (command === 'down' && root) {
  await down(root);
} else if (command === 'relaunch-app' && root) {
  await relaunchApp(root);
} else {
  console.error(
    'usage: e2e-native.mjs up | seed <root> | relaunch-app <root> | down <root>',
  );
  process.exit(2);
}
