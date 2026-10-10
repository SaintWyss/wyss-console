// What makes a release package acceptable, as functions that can be tested without
// building one. `release-verify.mjs` applies them to the artifacts that were produced and
// `tests/release.test.ts` applies them to packages that were made on purpose to be wrong.
//
// Nothing here prints a secret: a finding names a file and a kind, never the bytes that
// matched.
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, readdirSync, readlinkSync } from 'node:fs';
import { join, posix, sep } from 'node:path';

/**
 * @typedef {{
 *   path: string,
 *   absolute: string,
 *   type: 'file' | 'dir' | 'symlink' | 'other',
 *   mode: number,
 *   size: number,
 *   target?: string,
 * }} Entry
 */

/** @param {string | Buffer} bytes */
export const sha256Of = (bytes) =>
  createHash('sha256').update(bytes).digest('hex');

/** @param {string} path */
export const sha256File = (path) => sha256Of(readFileSync(path));

/**
 * Every entry below `root`, in a stable order, with paths relative to it and written
 * with `/`. Links are reported as links and never followed.
 * @param {string} root
 * @returns {Entry[]}
 */
export function walk(root) {
  /** @type {Entry[]} */
  const out = [];
  /** @param {string} directory */
  const visit = (directory) => {
    const names = readdirSync(directory).sort();
    for (const name of names) {
      const absolute = join(directory, name);
      const stat = lstatSync(absolute);
      const path = absolute
        .slice(root.length + 1)
        .split(sep)
        .join('/');
      /** @type {Entry['type']} */
      const type = stat.isSymbolicLink()
        ? 'symlink'
        : stat.isDirectory()
          ? 'dir'
          : stat.isFile()
            ? 'file'
            : 'other';
      /** @type {Entry} */
      const entry = {
        path,
        absolute,
        type,
        mode: stat.mode & 0o7777,
        size: stat.size,
      };
      if (type === 'symlink') entry.target = readlinkSync(absolute);
      out.push(entry);
      if (type === 'dir') visit(absolute);
    }
  };
  visit(root);
  return out;
}

/**
 * Names that must not be in a package: credentials, local state, development fixtures,
 * the repository's own working files, anything that came from a checkout.
 * @type {{ kind: string, test: RegExp }[]}
 */
export const FORBIDDEN_PATHS = [
  { kind: 'environment file', test: /(^|\/)\.env(\.[^/]*)?$/ },
  { kind: 'provider credentials', test: /(^|\/)auth\.json$/ },
  {
    kind: 'credentials file',
    test: /(^|\/)(credentials|secrets?)(\.[^/]*)?$/i,
  },
  {
    kind: 'private key',
    test: /(^|\/)(id_(rsa|dsa|ecdsa|ed25519)|[^/]*\.(pem|key|p12|pfx|jks|keystore))$/i,
  },
  {
    kind: 'package manager credentials',
    test: /(^|\/)\.(npmrc|netrc|pypirc|git-credentials)$/,
  },
  { kind: 'database', test: /\.(sqlite3?|db)(-wal|-shm)?$/i },
  { kind: 'source control', test: /(^|\/)\.git(\/|$)/ },
  {
    kind: 'build cache',
    test: /(^|\/)(\.cache|\.artifacts|node_modules|target)(\/|$)/,
  },
  {
    kind: 'development fixture',
    test: /(^|\/)(fixtures?|tests?|__tests__|examples?)(\/|$)/i,
  },
  {
    kind: 'test file',
    test: /\.(test|spec)\.[cm]?[jt]sx?$|(^|\/)codex_runtime\.py$/,
  },
  {
    kind: 'source file',
    test: /\.(rs|ts|svelte|mjs)$|(^|\/)(Cargo\.(toml|lock)|package(-lock)?\.json)$/,
  },
  {
    kind: 'acceptance state',
    test: /(^|\/)(acceptance|\.acceptance|\.wyss-test)[^/]*(\/|$)/i,
  },
];

/**
 * @param {readonly Entry[]} entries
 * @returns {{ path: string, kind: string }[]}
 */
export function forbiddenPaths(entries) {
  /** @type {{ path: string, kind: string }[]} */
  const found = [];
  for (const { path } of entries)
    for (const { kind, test } of FORBIDDEN_PATHS)
      if (test.test(path)) {
        found.push({ path, kind });
        break;
      }
  return found;
}

/**
 * Content that looks like a live credential. Deliberately narrow: each of these has a
 * shape nothing else has, so a match is worth stopping for.
 * @type {{ kind: string, pattern: RegExp, anchors: string[] }[]}
 */
export const SECRET_PATTERNS = [
  {
    kind: 'private key block',
    // The marker alone is in every TLS library, as a string to look for. A key has a
    // body of base64 after it, with optional headers in between.
    pattern:
      /-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY(?: BLOCK)?-----\r?\n(?:[A-Za-z-]+: [^\n]*\n)*\s*[A-Za-z0-9+/=]{40,}/,
    anchors: ['-----BEGIN '],
  },
  {
    kind: 'OpenAI-style API key',
    pattern: /\bsk-(?:proj-|live-|svcacct-)?[A-Za-z0-9_-]{32,}\b/,
    anchors: ['sk-'],
  },
  {
    kind: 'Anthropic-style API key',
    pattern: /\bsk-ant-[A-Za-z0-9_-]{24,}\b/,
    anchors: ['sk-ant-'],
  },
  {
    kind: 'GitHub token',
    pattern:
      /\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}\b|\bgithub_pat_[A-Za-z0-9_]{50,}\b/,
    anchors: ['ghp_', 'gho_', 'ghu_', 'ghs_', 'ghr_', 'github_pat_'],
  },
  {
    kind: 'AWS access key id',
    pattern: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/,
    anchors: ['AKIA', 'ASIA'],
  },
  {
    kind: 'Slack token',
    pattern: /\bxox[abprs]-[A-Za-z0-9-]{20,}\b/,
    anchors: ['xox'],
  },
  {
    kind: 'Google API key',
    pattern: /\bAIza[0-9A-Za-z_-]{35}\b/,
    anchors: ['AIza'],
  },
  {
    kind: 'JSON web token',
    pattern:
      /\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/,
    anchors: ['eyJ'],
  },
  {
    kind: 'bearer credential',
    pattern: /\b[Aa]uthorization:\s*[Bb]earer\s+[A-Za-z0-9._~+/-]{24,}/,
    anchors: ['uthorization:'],
  },
  {
    kind: 'stored login tokens',
    pattern:
      /"(?:refresh_token|access_token|id_token)"\s*:\s*"[A-Za-z0-9._~+/=-]{24,}"/,
    anchors: ['_token"'],
  },
  {
    kind: 'credentials in a URL',
    // A user and a password in a real address. The format strings (`%s:%d`) and the
    // examples that libraries print are not one.
    pattern:
      /\b[a-z][a-z0-9+.-]*:\/\/[A-Za-z0-9._~-]{1,64}:[A-Za-z0-9._~!$&'()*+,;=-]{3,64}@[a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,}\b/i,
    anchors: ['://'],
  },
  {
    kind: 'package registry token',
    pattern:
      /_authToken\s*=\s*[A-Za-z0-9._~+/=-]{8,}|:_auth\s*=\s*[A-Za-z0-9+/=]{8,}/,
    anchors: ['_auth'],
  },
];

/**
 * Files larger than this are not read as text. The authority and the desktop are a few
 * tens of megabytes; nothing in a package is larger than this.
 */
const MAX_SCAN = 600 * 1024 * 1024;

/** Most places one anchor is looked at in one file. */
const MAX_HITS = 50_000;
/**
 * @param {Buffer} bytes
 * @param {string} anchor
 * @param {RegExp} pattern
 */
function anchored(bytes, anchor, pattern) {
  const needle = Buffer.from(anchor, 'latin1');
  let at = bytes.indexOf(needle);
  for (let hits = 0; at !== -1 && hits < MAX_HITS; hits++) {
    const window = bytes.toString('latin1', Math.max(0, at - 12), at + 400);
    if (pattern.test(window)) return true;
    at = bytes.indexOf(needle, at + needle.length);
  }
  return false;
}

/**
 * @param {readonly Entry[]} entries
 * @returns {{ path: string, kind: string }[]}
 */
export function secretsIn(entries) {
  /** @type {{ path: string, kind: string }[]} */
  const found = [];
  for (const entry of entries) {
    if (entry.type !== 'file' || entry.size > MAX_SCAN) continue;
    // Binary or text, the same: a credential embedded in an executable is as bad. The
    // file is searched for the fixed words each shape starts with and only the bytes
    // around them are matched, so a package of hundreds of megabytes takes seconds.
    const bytes = readFileSync(entry.absolute);
    for (const { kind, pattern, anchors } of SECRET_PATTERNS)
      if (anchors.some((anchor) => anchored(bytes, anchor, pattern)))
        found.push({ path: entry.path, kind });
  }
  return found;
}

/**
 * Strings that tie a package to the machine or the checkout that made it. A package that
 * works only where it was built is not a package.
 * @param {readonly Entry[]} entries
 * @param {readonly string[]} needles absolute paths of the build host, longest first
 * @returns {{ path: string, needle: string }[]}
 */
export function buildHostReferences(entries, needles) {
  /** @type {{ path: string, needle: string }[]} */
  const found = [];
  for (const entry of entries) {
    if (entry.type !== 'file' || entry.size > MAX_SCAN) continue;
    const bytes = readFileSync(entry.absolute);
    for (const needle of needles)
      if (bytes.includes(Buffer.from(needle)))
        found.push({ path: entry.path, needle });
  }
  return found;
}

/**
 * Entries nobody but the owner may write, and none that is a set-id program. Anything
 * writable by a group or by others in a package is something another account could
 * have replaced.
 * @param {readonly Entry[]} entries
 * @returns {{ path: string, problem: string }[]}
 */
export function permissionProblems(entries) {
  /** @type {{ path: string, problem: string }[]} */
  const found = [];
  for (const { path, type, mode } of entries) {
    if (type === 'symlink') continue;
    if (mode & 0o022)
      found.push({ path, problem: 'writable by group or others' });
    if (mode & 0o6000) found.push({ path, problem: 'set-id' });
  }
  return found;
}

/**
 * Links that leave the tree they are in, or are absolute: a package is its own files.
 * @param {readonly Entry[]} entries
 * @returns {{ path: string, target: string }[]}
 */
export function escapingLinks(entries) {
  /** @type {{ path: string, target: string }[]} */
  const found = [];
  for (const entry of entries) {
    if (entry.type !== 'symlink' || entry.target === undefined) continue;
    const target = entry.target;
    const resolved = posix.normalize(
      posix.join(posix.dirname(entry.path), target),
    );
    if (posix.isAbsolute(target) || resolved.startsWith('..'))
      found.push({ path: entry.path, target });
  }
  return found;
}

/**
 * Where the desktop looks for its authority, below the prefix of its executable.
 * The same rule `Provision::of_build` applies in the desktop.
 * @param {string} executable path of the desktop, relative to the package root
 */
export function payloadFor(executable) {
  const prefix = posix.dirname(posix.dirname(executable));
  return posix.join(prefix, 'libexec', 'wyss-console', 'wyss-control');
}

/**
 * `sha256sum`'s output: one `<hex>  <name>` line per file.
 * @param {string} text
 * @returns {Map<string, string>}
 */
export function parseSums(text) {
  /** @type {Map<string, string>} */
  const sums = new Map();
  for (const line of text.split('\n')) {
    if (line === '') continue;
    const match = /^([0-9a-f]{64}) [ *]([^/\0\n]+)$/.exec(line);
    if (!match || sums.has(match[2] ?? ''))
      throw new Error('SHA256SUMS is not valid');
    sums.set(
      /** @type {string} */ (match[2]),
      /** @type {string} */ (match[1]),
    );
  }
  return sums;
}

/**
 * @param {ReadonlyMap<string, string>} sums
 * @returns {string}
 */
export function formatSums(sums) {
  return [...sums].map(([name, hex]) => `${hex}  ${name}\n`).join('');
}
