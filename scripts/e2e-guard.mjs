// Guards for the native end-to-end harness, as functions that can be tested without a
// window: where a disposable sandbox may live, what environment a launched product
// inherits, and whether a binary carries the Codex fixture backend.
//
// The fixture is `src-tauri/tests/fixtures/codex_runtime.py`. It only runs as a `codex`
// executable found on a sandbox PATH, never from production code and never through a
// runtime setting. These guards fail closed: when isolation cannot be shown, they throw.
import {
  copyFileSync,
  chmodSync,
  existsSync,
  lstatSync,
  realpathSync,
  readdirSync,
  readFileSync,
  statSync,
} from 'node:fs';
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
} from 'node:path';

/** Strings only the test fixture contains. A release binary must contain none of them. */
export const FIXTURE_MARKERS = [
  'WYSS_RUNTIME_FIXTURE',
  'WYSS_RUNTIME_SCENARIO',
  'codex_runtime.py',
];

/** Unix socket paths are limited to 108 bytes on Linux. */
export const SOCKET_PATH_LIMIT = 108;

/** The marker every native E2E state file carries. A file without it is not a harness state. */
export const NATIVE_STATE_MARKER = 'wyss-console-native-e2e-v1';

/** A display the harness may name: a colon and a number, nothing that can leave a path. */
const DISPLAY_NAME = /^:\d{1,4}$/;

/**
 * @param {string} child
 * @param {string} parent
 * @returns {boolean} true when `child` is `parent` or lies beneath it
 */
function within(child, parent) {
  const rel = relative(resolve(parent), resolve(child));
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
}

/**
 * Throws unless `root` is an absolute path that can hold a disposable sandbox: outside
 * the real home and the real user runtime, and short enough for the control socket.
 * @param {string} root
 * @param {{ home: string, uid: number }} real the real home directory and user id
 */
export function assertSandboxRoot(root, real) {
  if (!isAbsolute(root))
    throw new Error('sandbox root must be an absolute path');
  if (within(root, real.home)) {
    throw new Error('sandbox root is inside the real home directory');
  }
  if (within(root, `/run/user/${real.uid}`)) {
    throw new Error('sandbox root is inside the real user runtime directory');
  }
  const socket = join(root, 'run', 'wyss-console', 'control.sock');
  if (Buffer.byteLength(socket) > SOCKET_PATH_LIMIT) {
    throw new Error(`control socket path exceeds ${SOCKET_PATH_LIMIT} bytes`);
  }
}

/**
 * A destructive or stateful E2E command may target only a harness-owned sandbox,
 * created directly under /tmp by mkdtempSync('/tmp/wacc-'). assertSandboxRoot alone
 * is NOT sufficient: it intentionally accepts arbitrary directories outside HOME.
 *
 * Refuse symlinks, aliases, unexpected names, foreign ownership, and loose modes.
 * The caller must additionally validate the state.json harness marker before acting.
 * @param {string} root
 * @param {{home: string, uid: number}} real
 */
export function assertManagedSandbox(root, real) {
  assertSandboxRoot(root, real);
  const tmp = realpathSync('/tmp');
  if (
    root !== resolve(root) ||
    dirname(root) !== tmp ||
    !/^wacc-[A-Za-z0-9]{6}$/.test(basename(root))
  ) {
    throw new Error('not a canonical harness-created /tmp/wacc-* sandbox');
  }
  const stat = lstatSync(root);
  if (
    !stat.isDirectory() ||
    stat.isSymbolicLink() ||
    realpathSync(root) !== root ||
    stat.uid !== real.uid ||
    (stat.mode & 0o777) !== 0o700
  ) {
    throw new Error(
      'sandbox ownership, canonical path, or permissions are unsafe',
    );
  }
}

/**
 * Throws when `path` exists and is not a regular file. The harness reads and writes its state
 * and ledger files, and a symlink would move those operations to a file outside the sandbox.
 * A missing file passes: the caller decides whether it may be missing.
 * @param {string} path
 */
export function assertPlainFileOrAbsent(path) {
  let stat;
  try {
    stat = lstatSync(path);
  } catch (error) {
    if (/** @type {NodeJS.ErrnoException} */ (error).code === 'ENOENT') return;
    throw error;
  }
  if (!stat.isFile()) {
    throw new Error(
      `${basename(path)} is not a regular file; refusing to follow it`,
    );
  }
}

/**
 * Throws unless the sandbox's runtime directory is this user's private directory and every
 * path a service command reads through it is a plain directory or socket of its own. The
 * service manager and the bus are reached through `XDG_RUNTIME_DIR`, so a link there would
 * send `systemctl --user` to the real user's manager. Missing paths fail closed.
 * @param {string} root
 * @param {{ uid: number }} real the real user id
 * @param {{ requireManager?: boolean }} [options] demand a live private socket before systemctl
 */
export function assertOwnedRuntime(root, real, { requireManager = false } = {}) {
  // Use lstat, not existsSync: a dangling symlink is still unsafe.
  const statOrNull = (path) => {
    try {
      return lstatSync(path);
    } catch (error) {
      if (/** @type {NodeJS.ErrnoException} */ (error).code === 'ENOENT')
        return null;
      throw error;
    }
  };
  const run = join(root, 'run');
  const runStat = lstatSync(run);
  if (
    !runStat.isDirectory() ||
    runStat.isSymbolicLink() ||
    runStat.uid !== real.uid ||
    (runStat.mode & 0o777) !== 0o700 ||
    realpathSync(run) !== run
  ) {
    throw new Error(
      'runtime directory is not a private directory owned by this user',
    );
  }
  const busStat = statOrNull(join(run, 'bus'));
  if (busStat && (!busStat.isSocket() || busStat.uid !== real.uid)) {
    throw new Error('session bus of the sandbox is not a socket owned by this user');
  }
  const systemd = join(run, 'systemd');
  const systemdStat = statOrNull(systemd);
  if (
    systemdStat &&
    (systemdStat.isSymbolicLink() ||
      !systemdStat.isDirectory() ||
      systemdStat.uid !== real.uid)
  ) {
    throw new Error('service manager directory of the sandbox is not an owned plain directory');
  }
  if (requireManager && !systemdStat) {
    throw new Error('sandbox service manager directory is missing');
  }
  // systemctl --user reaches this socket. Checking the parent directory alone
  // does not prevent a symlink to /run/user/<uid>/systemd/private.
  const privateStat = systemdStat ? statOrNull(join(systemd, 'private')) : null;
  if (
    (requireManager && !privateStat) ||
    (privateStat && (!privateStat.isSocket() || privateStat.uid !== real.uid))
  ) {
    throw new Error('sandbox private manager socket is missing or unsafe');
  }
}

/**
 * Throws unless a parsed state file is this harness's own state for `root`. Every field that
 * a cleanup later turns into a path or a signal is checked here, before it is used.
 * @param {unknown} state
 * @param {string} root
 */
export function assertNativeState(state, root) {
  const fail = () => {
    throw new Error('invalid or foreign native E2E sandbox state');
  };
  if (typeof state !== 'object' || state === null) fail();
  const record = /** @type {Record<string, unknown>} */ (state);
  if (
    record.harness !== NATIVE_STATE_MARKER ||
    record.root !== root ||
    record.workspace !== join(root, 'workspace')
  ) {
    fail();
  }
  if (
    record.display !== undefined &&
    (typeof record.display !== 'string' || !DISPLAY_NAME.test(record.display))
  ) {
    fail();
  }
  const positive = (/** @type {unknown} */ value) =>
    Number.isSafeInteger(value) && /** @type {number} */ (value) > 0;
  if (record.pids !== undefined) {
    if (typeof record.pids !== 'object' || record.pids === null) fail();
    if (
      !Object.values(
        /** @type {Record<string, unknown>} */ (record.pids),
      ).every(positive)
    ) {
      fail();
    }
  }
  if (record.groups !== undefined) {
    if (!Array.isArray(record.groups) || !record.groups.every(positive)) fail();
  }
}

/**
 * The complete environment a launched product or authority may inherit. Nothing from the
 * real session is included: no real D-Bus address, no real home, no CODEX_HOME.
 * @param {{ root: string, fixtureDir: string, scenario: string, display: string }} options
 * @returns {Record<string, string>}
 */
export function sandboxEnv({ root, fixtureDir, scenario, display }) {
  /** @type {Record<string, string>} */
  const env = {
    HOME: join(root, 'home'),
    XDG_RUNTIME_DIR: join(root, 'run'),
    XDG_CONFIG_HOME: join(root, 'cfg'),
    XDG_DATA_HOME: join(root, 'data'),
    XDG_STATE_HOME: join(root, 'state'),
    XDG_CACHE_HOME: join(root, 'cache'),
    PATH: `${join(root, 'bin')}:/usr/bin:/bin`,
    LANG: 'C.UTF-8',
    GDK_BACKEND: 'x11',
    NO_AT_BRIDGE: '1',
    CODEX_HOME: join(root, 'codex-home'),
    WYSS_FIXTURE_USER_HOME: join(root, 'codex-home'),
    WYSS_RUNTIME_FIXTURE: fixtureDir,
    WYSS_RUNTIME_SCENARIO: scenario,
  };
  // A display is named only by the processes that draw on it: the compositor starts the
  // server it serves, so it must not be pointed at that server before it exists.
  if (display) env.DISPLAY = display;
  return env;
}

/**
 * Installs the repository fixture as the sandbox's `codex` executable. Only this copy is
 * ever run; the file in the repository is read, never executed.
 * @param {string} root
 * @param {string} fixturePath
 * @returns {string} the installed path
 */
export function installFixtureCodex(root, fixturePath) {
  const target = join(root, 'bin', 'codex');
  copyFileSync(fixturePath, target);
  chmodSync(target, 0o700);
  return target;
}

/**
 * @param {Uint8Array} bytes
 * @returns {string[]} the fixture markers this binary contains
 */
export function fixtureHits(bytes) {
  const text = Buffer.from(bytes).toString('latin1');
  return FIXTURE_MARKERS.filter((marker) => text.includes(marker));
}

/**
 * Every file named like a product binary under `dir`, recursively.
 * @param {string} dir
 * @returns {string[]}
 */
export function productBinaries(dir) {
  const found = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      found.push(...productBinaries(path));
    } else if (entry === 'wyss-console' || entry === 'wyss-control') {
      found.push(path);
    }
  }
  return found;
}

/**
 * @param {string} path
 * @returns {string[]} the fixture markers found in the file
 */
export function fixtureHitsInFile(path) {
  return fixtureHits(readFileSync(path));
}
