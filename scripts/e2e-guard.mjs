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
  lstatSync,
  realpathSync,
  readdirSync,
  readFileSync,
  statSync,
} from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';

/** Strings only the test fixture contains. A release binary must contain none of them. */
export const FIXTURE_MARKERS = [
  'WYSS_RUNTIME_FIXTURE',
  'WYSS_RUNTIME_SCENARIO',
  'codex_runtime.py',
];

/** Unix socket paths are limited to 108 bytes on Linux. */
export const SOCKET_PATH_LIMIT = 108;

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
    throw new Error('sandbox ownership, canonical path, or permissions are unsafe');
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
