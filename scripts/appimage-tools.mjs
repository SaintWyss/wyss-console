// The executables the AppImage build runs that are not built here, and how they are
// made trustworthy before anything runs them.
//
//   fixed https address of an immutable release  ->  bounded download to a name nothing
//   else uses  ->  SHA-256 of what arrived  ->  (the one byte change Tauri would make)
//   ->  SHA-256 of what will run  ->  renamed into place  ->  checked again after the
//   bundler used it.
//
// The expected digests are committed in `appimage-tools.json`. Nothing here trusts a
// file name, a version or a file that was already in the cache: every use hashes what is
// there, and a file that is not exactly the pinned one is removed and fetched again, or
// the build stops. There is no second address to fall back to and no `PATH` lookup.
//
// Nothing in this file touches the network unless it is given the default fetcher, and
// the tests give it a fake one.
import { spawnSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import {
  chmodSync,
  closeSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeSync,
} from 'node:fs';
import { join } from 'node:path';

/**
 * @typedef {{
 *   name: string,
 *   file: string,
 *   version: string,
 *   source: string,
 *   download_sha256: string,
 *   sha256: string,
 *   max_bytes: number,
 *   transform: null | 'clear-appimage-magic',
 * }} Tool
 * @typedef {{ name: string, file: string, provider: string, sha256: string }} Embedded
 * @typedef {{ schema_version: 1, tools: Tool[], embedded: Embedded[] }} ToolsConfig
 * @typedef {(url: string, destination: string, maxBytes: number) => void} Fetch
 */

const HEX = /^[0-9a-f]{64}$/;
/** The hosts a release asset of GitHub is served from. A redirect elsewhere is refused. */
export const ALLOWED_HOSTS = new Set([
  'github.com',
  'objects.githubusercontent.com',
  'release-assets.githubusercontent.com',
]);
/** A reference that names "whatever is newest" is not a version. */
const FLOATING =
  /(^|[/@._-])(continuous|latest|nightly|stable|head|master|main|edge|current)([/@._-]|$)/i;

/** @param {Buffer | string} bytes */
export const sha256Of = (bytes) =>
  createHash('sha256').update(bytes).digest('hex');

/**
 * Refuses a configuration that could name a floating or unverified tool. Returns the
 * problems; an empty list is a configuration that pins every tool.
 * @param {unknown} config
 * @returns {string[]}
 */
export function configProblems(config) {
  /** @type {string[]} */
  const problems = [];
  const c = /** @type {Partial<ToolsConfig>} */ (config);
  if (typeof c !== 'object' || c === null || c.schema_version !== 1)
    return ['the configuration is not schema version 1'];
  if (!Array.isArray(c.tools) || c.tools.length === 0)
    problems.push('no tool is listed');
  const files = new Set();
  for (const tool of c.tools ?? []) {
    const at = `tool ${tool?.name ?? '?'}`;
    if (typeof tool?.name !== 'string' || tool.name === '')
      problems.push(`${at}: no name`);
    if (
      typeof tool?.file !== 'string' ||
      !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(tool.file)
    )
      problems.push(`${at}: the file name is not a plain name`);
    else if (files.has(tool.file))
      problems.push(`${at}: the file name is repeated`);
    else files.add(tool.file);
    if (
      typeof tool?.version !== 'string' ||
      tool.version === '' ||
      FLOATING.test(tool.version)
    )
      problems.push(
        `${at}: the version is missing or names whatever is newest`,
      );
    let url;
    try {
      url = new URL(String(tool?.source));
    } catch {
      problems.push(`${at}: the source is not an address`);
    }
    if (url) {
      const m = /^\/[^/]+\/[^/]+\/releases\/download\/([^/]+)\/[^/]+$/.exec(
        url.pathname,
      );
      if (
        url.protocol !== 'https:' ||
        url.hostname !== 'github.com' ||
        url.username ||
        url.password ||
        url.port ||
        url.search ||
        url.hash ||
        !m
      )
        problems.push(
          `${at}: the source is not a fixed https release asset of github.com`,
        );
      else if (FLOATING.test(m[1] ?? '') || FLOATING.test(url.pathname))
        problems.push(`${at}: the source names a floating release`);
      else if (!(tool?.version && (m[1] ?? '').includes(tool.version)))
        problems.push(`${at}: the version is not the release the source names`);
    }
    if (!HEX.test(String(tool?.download_sha256)))
      problems.push(`${at}: no SHA-256 for what is downloaded`);
    if (!HEX.test(String(tool?.sha256)))
      problems.push(`${at}: no SHA-256 for what runs`);
    if (tool?.transform !== null && tool?.transform !== 'clear-appimage-magic')
      problems.push(`${at}: an unknown transform`);
    if (tool?.transform === null && tool.sha256 !== tool.download_sha256)
      problems.push(`${at}: unchanged, so what runs is what is downloaded`);
    if (
      !Number.isSafeInteger(tool?.max_bytes) ||
      tool.max_bytes <= 0 ||
      tool.max_bytes > 256 * 1024 * 1024
    )
      problems.push(`${at}: no sensible size bound`);
  }
  for (const e of c.embedded ?? []) {
    const at = `embedded ${e?.name ?? '?'}`;
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(String(e?.file)))
      problems.push(`${at}: the file name is not a plain name`);
    else if (files.has(e.file))
      problems.push(`${at}: the file name is repeated`);
    else files.add(e.file);
    if (!HEX.test(String(e?.sha256))) problems.push(`${at}: no SHA-256`);
  }
  return problems;
}

/**
 * What Tauri does to linuxdeploy before it runs it: the AppImage magic bytes are zeroed,
 * so that it can be run without FUSE. Done here, to a file whose digest is known, so
 * that the bundler's own change leaves the file as it is.
 * @param {Tool['transform']} transform
 * @param {Buffer} bytes
 */
export function transformed(transform, bytes) {
  if (transform === null) return bytes;
  const magic = Buffer.from([0x41, 0x49, 0x02]);
  if (bytes.length < 11 || !bytes.subarray(8, 11).equals(magic))
    throw new Error('the file is not an AppImage of the expected type');
  const copy = Buffer.from(bytes);
  copy.fill(0, 8, 11);
  return copy;
}

/**
 * Hashes `path` the way it is used: a regular file, never a link, within its bound.
 * @param {string} path
 * @param {string} expected
 * @param {number} maxBytes
 * @returns {'ok' | 'missing' | 'not-a-file' | 'too-large' | 'mismatch'}
 */
export function checkFile(path, expected, maxBytes) {
  let stat;
  try {
    stat = lstatSync(path);
  } catch {
    return 'missing';
  }
  if (!stat.isFile()) return 'not-a-file';
  if (stat.size > maxBytes) return 'too-large';
  return sha256Of(readFileSync(path)) === expected ? 'ok' : 'mismatch';
}

/** @param {string} path */
function remove(path) {
  // A link is removed, never what it points at; a directory is not trusted either.
  rmSync(path, { recursive: true, force: true });
}

/**
 * Makes `path` exactly the pinned tool, or throws and leaves nothing in its place.
 * @param {Tool} tool
 * @param {string} directory
 * @param {Fetch} fetch
 * @returns {'cached' | 'fetched'}
 */
export function provision(tool, directory, fetch) {
  const target = join(directory, tool.file);
  if (checkFile(target, tool.sha256, tool.max_bytes) === 'ok') return 'cached';
  // Whatever is there is not the tool: a different version, a damaged file, a link, a
  // directory. It is removed rather than trusted.
  remove(target);
  const suffix = `${process.pid}.${randomBytes(6).toString('hex')}`;
  const download = join(directory, `.${tool.file}.${suffix}.part`);
  const staged = join(directory, `.${tool.file}.${suffix}.run`);
  try {
    fetch(tool.source, download, tool.max_bytes);
    // What arrived: bounded, whole, and the release that was named.
    const state = checkFile(download, tool.download_sha256, tool.max_bytes);
    if (state !== 'ok')
      throw new Error(
        `${tool.name}: what was downloaded is not the pinned release (${state})`,
      );
    const bytes = transformed(tool.transform, readFileSync(download));
    if (sha256Of(bytes) !== tool.sha256)
      throw new Error(`${tool.name}: what would run is not the pinned file`);
    // Written to a name of its own, executable, flushed, then renamed into place: a
    // partial file is never at the name the bundler looks for.
    const fd = openSync(staged, 'wx', 0o755);
    try {
      writeSync(fd, bytes);
      fsyncSync(fd);
    } finally {
      closeSync(fd);
    }
    chmodSync(staged, 0o755);
    renameSync(staged, target);
  } finally {
    remove(download);
    remove(staged);
  }
  if (checkFile(target, tool.sha256, tool.max_bytes) !== 'ok') {
    remove(target);
    throw new Error(`${tool.name}: the file in place is not the pinned one`);
  }
  return 'fetched';
}

/**
 * Empties `directory` of everything that is not a pinned download, creates it private,
 * and makes every pinned tool exactly what is pinned. What the bundler writes itself is
 * removed first, so that what it finds is only what was verified here.
 * @param {ToolsConfig} config
 * @param {string} directory
 * @param {Fetch} fetch
 * @returns {{ name: string, how: 'cached' | 'fetched' }[]}
 */
export function provisionAll(config, directory, fetch) {
  const problems = configProblems(config);
  if (problems.length > 0)
    throw new Error(
      `the tool configuration is not pinned: ${problems.join('; ')}`,
    );
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  chmodSync(directory, 0o700);
  const keep = new Set(config.tools.map((t) => t.file));
  for (const name of readdirSync(directory)) {
    if (!keep.has(name)) remove(join(directory, name));
  }
  return config.tools.map((tool) => ({
    name: tool.name,
    how: provision(tool, directory, fetch),
  }));
}

/**
 * What is wrong with `directory` as a set of build tools. Before the bundler runs only
 * the pinned downloads are expected; after it, the two scripts it writes as well.
 * @param {ToolsConfig} config
 * @param {string} directory
 * @param {'before' | 'after'} phase
 * @returns {string[]}
 */
export function directoryProblems(config, directory, phase) {
  /** @type {string[]} */
  const problems = [];
  const expected = new Map(
    config.tools.map((t) => [
      t.file,
      { sha: t.sha256, max: t.max_bytes, name: t.name },
    ]),
  );
  if (phase === 'after')
    for (const e of config.embedded)
      expected.set(e.file, { sha: e.sha256, max: 1024 * 1024, name: e.name });
  let present;
  try {
    present = readdirSync(directory);
  } catch {
    return ['the tool directory is missing'];
  }
  for (const name of present)
    if (!expected.has(name)) problems.push(`${name}: not a pinned tool`);
  for (const [file, { sha, max, name }] of expected) {
    const state = checkFile(join(directory, file), sha, max);
    if (state !== 'ok') problems.push(`${name} (${file}): ${state}`);
  }
  return problems;
}

/**
 * The text the bundler printed, searched for a download: with every tool in place it has
 * none to make, so one that appears means something else was fetched and run.
 * @param {string} text
 */
export function downloadsIn(text) {
  return text
    .split('\n')
    .filter((line) => /\bDownloading\b/i.test(line))
    .map((line) => line.trim());
}

/**
 * The environment the bundler runs in: no proxy route. Every proxy variable names an
 * address nothing listens on and Cargo is told not to fetch, so a download by a client that
 * honours them fails instead of running. This does not stop a client that connects
 * directly; the pinned tool set, hashed before and after, and the scan of the bundler's
 * output for a download are what catch that.
 * @param {NodeJS.ProcessEnv} env
 * @returns {NodeJS.ProcessEnv}
 */
export function offlineEnvironment(env) {
  const dead = 'http://127.0.0.1:9';
  return {
    ...env,
    HTTPS_PROXY: dead,
    https_proxy: dead,
    HTTP_PROXY: dead,
    http_proxy: dead,
    ALL_PROXY: dead,
    all_proxy: dead,
    NO_PROXY: '',
    no_proxy: '',
    CARGO_NET_OFFLINE: 'true',
  };
}

/**
 * The default fetcher: `curl`, by its fixed path, over https only, redirects only to the
 * hosts that serve release assets, bounded in size and time, to a name that does not
 * exist yet.
 * @type {Fetch}
 */
export function curlFetch(url, destination, maxBytes) {
  const curl = '/usr/bin/curl';
  const result = spawnSync(
    curl,
    [
      '--fail',
      '--silent',
      '--show-error',
      '--location',
      '--proto',
      '=https',
      '--proto-redir',
      '=https',
      '--tlsv1.2',
      '--max-redirs',
      '5',
      '--max-filesize',
      String(maxBytes),
      '--max-time',
      '300',
      '--output',
      destination,
      '--write-out',
      '%{url_effective}',
      url,
    ],
    { encoding: 'utf8' },
  );
  if (result.error || result.status !== 0)
    throw new Error(
      `download failed: ${String(result.stderr).trim() || result.status}`,
    );
  let host = '';
  try {
    host = new URL(result.stdout.trim()).hostname;
  } catch {
    // an effective address that is not one is refused below
  }
  if (!ALLOWED_HOSTS.has(host)) {
    remove(destination);
    throw new Error(`download was served from ${host || 'an unknown host'}`);
  }
}

/**
 * What the manifest says about the tools: the pin and what is on disk.
 * @param {ToolsConfig} config
 * @param {string} directory
 */
export function describeTools(config, directory) {
  return {
    downloaded: config.tools.map((t) => ({
      name: t.name,
      version: t.version,
      source: t.source,
      download_sha256: t.download_sha256,
      expected_sha256: t.sha256,
      actual_sha256: sha256Of(readFileSync(join(directory, t.file))),
    })),
    embedded: config.embedded.map((e) => ({
      name: e.name,
      provider: e.provider,
      expected_sha256: e.sha256,
      actual_sha256: sha256Of(readFileSync(join(directory, e.file))),
    })),
  };
}
