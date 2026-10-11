// What makes a release package acceptable, as functions that can be tested without
// building one. `release-verify.mjs` applies them to the artifacts that were produced and
// `tests/release.test.ts` applies them to packages that were made on purpose to be wrong.
//
// Nothing here prints a secret: a finding names a file and a kind, never the bytes that
// matched.
import { spawnSync } from 'node:child_process';
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
 * @param {readonly { path: string }[]} entries
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
 * Files larger than this are not read as text, and are therefore reported rather than
 * passed: a scanner that skips what it cannot read has certified nothing about it. The
 * authority and the desktop are a few tens of megabytes; nothing in a package is larger.
 */
const MAX_SCAN = 600 * 1024 * 1024;

/**
 * Most places one anchor is looked at in one file. It bounds the work, and reaching it is
 * a finding, not a clean result: what lies after the last place looked at was not read.
 */
const MAX_HITS = 5_000_000;
/**
 * @param {Buffer} bytes
 * @param {string} anchor
 * @param {RegExp} pattern
 * @param {number} maxHits
 * @returns {'found' | 'clear' | 'exhausted'}
 */
function anchored(bytes, anchor, pattern, maxHits) {
  const needle = Buffer.from(anchor, 'latin1');
  let at = bytes.indexOf(needle);
  for (let hits = 0; at !== -1; hits++) {
    if (hits >= maxHits) return 'exhausted';
    const window = bytes.toString('latin1', Math.max(0, at - 12), at + 400);
    if (pattern.test(window)) return 'found';
    at = bytes.indexOf(needle, at + needle.length);
  }
  return 'clear';
}

/**
 * The kinds of credential-shaped content in `bytes`, and the kinds whose scan reached its
 * limit (reported as findings too: what lies after the last place looked at was not read).
 * Binary or text, the same: a credential embedded in an executable is as bad. The bytes
 * are searched for the fixed words each shape starts with and only the bytes around them
 * are matched, so a package of hundreds of megabytes takes seconds.
 * @param {Buffer} bytes
 * @param {number} [maxHits]
 * @returns {string[]}
 */
export function secretKindsIn(bytes, maxHits = MAX_HITS) {
  /** @type {string[]} */
  const kinds = [];
  for (const { kind, pattern, anchors } of SECRET_PATTERNS) {
    const results = anchors.map((anchor) =>
      anchored(bytes, anchor, pattern, maxHits),
    );
    if (results.includes('found')) kinds.push(kind);
    else if (results.includes('exhausted'))
      kinds.push(`${kind} (scan limit reached)`);
  }
  return kinds;
}

/**
 * Credential-shaped content in the files of a package or a checkout. A file that cannot be
 * scanned (too large, or too many places to look at) is reported as such: it fails the
 * release like a credential does.
 * @param {readonly Entry[]} entries
 * @param {{ maxBytes?: number, maxHits?: number }} [limits]
 * @returns {{ path: string, kind: string }[]}
 */
export function secretsIn(entries, limits = {}) {
  const maxBytes = limits.maxBytes ?? MAX_SCAN;
  const maxHits = limits.maxHits ?? MAX_HITS;
  /** @type {{ path: string, kind: string }[]} */
  const found = [];
  for (const entry of entries) {
    if (entry.type !== 'file') continue;
    if (entry.size > maxBytes) {
      found.push({ path: entry.path, kind: 'file too large to scan' });
      continue;
    }
    for (const kind of secretKindsIn(readFileSync(entry.absolute), maxHits))
      found.push({ path: entry.path, kind });
  }
  return found;
}

/**
 * @param {string} where
 * @param {string[]} args
 * @param {Buffer} [input]
 */
function git(where, args, input) {
  return spawnSync('git', ['-C', where, ...args], {
    input,
    maxBuffer: 2 * MAX_SCAN,
  });
}

/** @param {Buffer} out NUL-separated records, as `-z` writes them */
const records = (out) =>
  out
    .toString('utf8')
    .split('\0')
    .filter((r) => r !== '');

/**
 * Git's own record of what a checkout tracks, read from its objects: every entry of the
 * committed tree (`HEAD`) and of the index. Whatever cannot be listed is a problem.
 * @param {string} where
 */
function trackedRecords(where) {
  /** @type {{ path: string, mode: string, type: string, oid: string }[]} */
  const found = [];
  /** @type {{ path: string, problem: string }[]} */
  const problems = [];
  const head = git(where, ['rev-parse', '--verify', '-q', 'HEAD^{commit}']);
  if (head.status !== 0)
    problems.push({ path: 'HEAD', problem: 'there is no commit to scan' });
  else {
    const tree = git(where, ['ls-tree', '-r', '-z', '--full-tree', 'HEAD']);
    if (tree.status !== 0)
      problems.push({
        path: 'HEAD',
        problem: 'the committed tree cannot be listed',
      });
    else
      for (const record of records(tree.stdout)) {
        const m = /^(\d{6}) (\w+) ([0-9a-f]{40,64})\t(.+)$/s.exec(record);
        if (m)
          found.push({
            mode: /** @type {string} */ (m[1]),
            type: /** @type {string} */ (m[2]),
            oid: /** @type {string} */ (m[3]),
            path: /** @type {string} */ (m[4]),
          });
        else
          problems.push({ path: 'HEAD', problem: 'an entry cannot be read' });
      }
  }
  const index = git(where, ['ls-files', '-s', '-z']);
  if (index.status !== 0)
    problems.push({ path: 'index', problem: 'the index cannot be listed' });
  else
    for (const record of records(index.stdout)) {
      const m = /^(\d{6}) ([0-9a-f]{40,64}) ([0-3])\t(.+)$/s.exec(record);
      if (!m) {
        problems.push({ path: 'index', problem: 'an entry cannot be read' });
        continue;
      }
      const [, mode, oid, stage, path] = /** @type {string[]} */ (m);
      if (stage !== '0')
        problems.push({ path, problem: 'unmerged in the index' });
      found.push({
        mode,
        oid,
        path,
        type: mode === '160000' ? 'commit' : 'blob',
      });
    }
  return { found, problems };
}

/**
 * The content of each object, read from Git's store (`git cat-file --batch`), never from
 * the working tree. An object larger than `maxBytes` is sized and not read.
 * @param {string} where
 * @param {readonly string[]} oids
 * @param {number} maxBytes
 * @returns {Map<string, { size: number, bytes?: Buffer }>} only the objects that could be read
 */
function blobContents(where, oids, maxBytes) {
  /** @type {Map<string, { size: number, bytes?: Buffer }>} */
  const out = new Map();
  if (oids.length === 0) return out;
  /** @param {string[]} args @param {readonly string[]} ask */
  const batch = (args, ask) => {
    const result = git(where, args, Buffer.from(`${ask.join('\n')}\n`));
    return result.status === 0 ? /** @type {Buffer} */ (result.stdout) : null;
  };
  /** @type {string[]} */
  const small = [];
  const sizes = batch(['cat-file', '--batch-check'], oids);
  if (sizes === null) return out;
  for (const line of sizes.toString('utf8').split('\n')) {
    const m = /^([0-9a-f]{40,64}) blob (\d+)$/.exec(line);
    if (!m) continue;
    const [oid, size] = [/** @type {string} */ (m[1]), Number(m[2])];
    if (size > maxBytes) out.set(oid, { size });
    else small.push(oid);
  }
  const content = small.length ? batch(['cat-file', '--batch'], small) : null;
  if (content === null) return out;
  // `<oid> blob <size>\n<content>\n`, once per object asked for, in order.
  let at = 0;
  for (const oid of small) {
    /** @type {number} */
    const eol = content.indexOf(0x0a, at);
    if (eol === -1) break;
    const m = /^([0-9a-f]{40,64}) blob (\d+)$/.exec(
      content.toString('utf8', at, eol),
    );
    const size = Number(m?.[2]);
    const end = eol + 1 + size;
    if (!m || m[1] !== oid || end >= content.length || content[end] !== 0x0a)
      break;
    out.set(oid, { size, bytes: content.subarray(eol + 1, end) });
    at = end + 1;
  }
  return out;
}

/**
 * The credential scan of a checkout. Two things, kept apart:
 *
 * - **What Git records**: every file of the committed tree and of the index is read from
 *   Git's objects and scanned. A file that was deleted, replaced or hidden in the working
 *   tree is still scanned as it was committed.
 * - **What is on disk**: every tracked path has to be in the working tree as the kind of
 *   entry Git records (a regular file, or a link, never followed), and a regular file is
 *   scanned as it is there too.
 *
 * Whatever cannot be listed, read or scanned is a problem: the scan fails closed. There is
 * no exception by path or by kind.
 * @param {string} where
 * @param {{ maxBytes?: number, maxHits?: number }} [limits]
 * @returns {{ paths: string[], secrets: { path: string, kind: string }[], problems: { path: string, problem: string }[] }}
 */
export function scanCheckout(where, limits = {}) {
  const maxBytes = limits.maxBytes ?? MAX_SCAN;
  const maxHits = limits.maxHits ?? MAX_HITS;
  const { found, problems } = trackedRecords(where);
  /** @type {Map<string, { path: string, kind: string }>} */
  const secrets = new Map();
  /** @param {string} path @param {string} kind */
  const secret = (path, kind) =>
    secrets.set(`${path}\0${kind}`, { path, kind });

  const blobs = found.filter((r) => r.type === 'blob');
  const contents = blobContents(
    where,
    [...new Set(blobs.map((r) => r.oid))],
    maxBytes,
  );
  for (const record of found) {
    if (record.type !== 'blob') {
      problems.push({
        path: record.path,
        problem: `a ${record.type === 'commit' ? 'submodule' : record.type}, whose content is not scanned`,
      });
      continue;
    }
    const blob = contents.get(record.oid);
    if (blob === undefined)
      problems.push({
        path: record.path,
        problem: 'its committed content cannot be read',
      });
    else if (blob.bytes === undefined)
      secret(record.path, 'file too large to scan');
    else
      for (const kind of secretKindsIn(blob.bytes, maxHits))
        secret(record.path, kind);
  }

  // The index's mode wins over the commit's: it is what the working tree is meant to hold.
  /** @type {Map<string, string>} */
  const modes = new Map();
  for (const r of found) if (r.type === 'blob') modes.set(r.path, r.mode);
  for (const [path, mode] of modes) {
    const absolute = join(where, ...path.split('/'));
    /** @type {import('node:fs').Stats} */
    let stat;
    try {
      stat = lstatSync(absolute);
    } catch {
      problems.push({
        path,
        problem: 'tracked but missing from the working tree',
      });
      continue;
    }
    if (mode === '120000') {
      if (!stat.isSymbolicLink()) {
        problems.push({
          path,
          problem: 'tracked as a link and not one on disk',
        });
        continue;
      }
      // A link's content, for Git, is its target: that is scanned, the link never followed.
      /** @type {Buffer} */
      let target;
      try {
        target = readlinkSync(absolute, { encoding: 'buffer' });
      } catch {
        problems.push({ path, problem: 'unreadable in the working tree' });
        continue;
      }
      for (const kind of secretKindsIn(target, maxHits)) secret(path, kind);
      continue;
    }
    if (!stat.isFile()) {
      problems.push({
        path,
        problem: 'tracked as a file and not a regular file on disk',
      });
      continue;
    }
    if (stat.size > maxBytes) {
      secret(path, 'file too large to scan');
      continue;
    }
    /** @type {Buffer} */
    let bytes;
    try {
      bytes = readFileSync(absolute);
    } catch {
      problems.push({ path, problem: 'unreadable in the working tree' });
      continue;
    }
    for (const kind of secretKindsIn(bytes, maxHits)) secret(path, kind);
  }
  return {
    paths: [...new Set(found.map((r) => r.path))].sort(),
    secrets: [...secrets.values()],
    problems,
  };
}

/**
 * Paths that only a build machine has, whichever machine that was: a home directory,
 * Cargo's or rustup's own directories, a Cargo target directory. A release names the
 * checkout and Cargo's home by fixed, made-up paths (`/build/wyss-console`, `/build/cargo`),
 * so a program that names one of these leaks the machine that built it, or was not built
 * the way a release is. Unlike the checkout's own path, these do not depend on where the
 * verifier happens to run.
 * @type {{ kind: string, pattern: RegExp, anchors: string[] }[]}
 */
export const BUILD_PATH_PATTERNS = [
  {
    kind: 'a home directory',
    pattern: /\/(?:home|Users)\/[A-Za-z0-9._-]+\//,
    anchors: ['/home/', '/Users/'],
  },
  {
    kind: "root's home directory",
    pattern: /(?:^|[^A-Za-z0-9._-])\/root\//,
    anchors: ['/root/'],
  },
  {
    kind: 'a Cargo home',
    pattern: /\/\.cargo\/(?:registry|git)\//,
    anchors: ['/.cargo/'],
  },
  {
    kind: 'a rustup toolchain',
    pattern: /\/\.rustup\/toolchains\//,
    anchors: ['/.rustup/'],
  },
  {
    kind: 'a Cargo target directory',
    pattern:
      /\/target\/(?:[a-z0-9_]+-[a-z0-9_]+-[a-z0-9_]+(?:-[a-z0-9_]+)?\/)?(?:release|debug)\/(?:build|deps)\//,
    anchors: ['/target/'],
  },
];

/**
 * Strings that tie a package to the machine or the checkout that made it. A package that
 * works only where it was built is not a package. `needles` are the paths this host knows
 * (its checkout, its scratch directory); `BUILD_PATH_PATTERNS` are those of any build host.
 * @param {readonly Entry[]} entries
 * @param {readonly string[]} needles absolute paths of the build host, longest first
 * @param {number} [maxBytes]
 * @returns {{ path: string, needle: string }[]}
 */
export function buildHostReferences(entries, needles, maxBytes = MAX_SCAN) {
  /** @type {{ path: string, needle: string }[]} */
  const found = [];
  for (const entry of entries) {
    if (entry.type !== 'file') continue;
    if (entry.size > maxBytes) {
      found.push({ path: entry.path, needle: '(file too large to scan)' });
      continue;
    }
    const bytes = readFileSync(entry.absolute);
    for (const needle of needles)
      if (bytes.includes(Buffer.from(needle)))
        found.push({ path: entry.path, needle });
    for (const { kind, pattern, anchors } of BUILD_PATH_PATTERNS) {
      const results = anchors.map((anchor) =>
        anchored(bytes, anchor, pattern, MAX_HITS),
      );
      if (results.includes('found'))
        found.push({ path: entry.path, needle: kind });
      else if (results.includes('exhausted'))
        found.push({
          path: entry.path,
          needle: `${kind} (scan limit reached)`,
        });
    }
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

/**
 * A release is built from a commit, and from nothing that is not in it.
 * @param {string | null} porcelain `git status --porcelain`, or null if git failed
 * @param {string | null} head `git rev-parse HEAD`, or null if git failed
 * @returns {string[]} what is wrong; empty means the tree is the commit
 */
export function sourceProblems(porcelain, head) {
  /** @type {string[]} */
  const problems = [];
  if (head === null || !/^[0-9a-f]{40}$/.test(head))
    problems.push('the commit being built cannot be named');
  if (porcelain === null)
    problems.push('the state of the working tree cannot be read');
  else if (porcelain.trim() !== '')
    problems.push(
      'the working tree has uncommitted changes; a release is built from a commit',
    );
  return problems;
}

/**
 * @param {{ revision?: unknown, revision_short?: unknown }} manifest
 * @param {string} head the full commit of the source it is compared with
 * @returns {string[]}
 */
export function revisionProblems(manifest, head) {
  /** @type {string[]} */
  const problems = [];
  if (manifest.revision !== head)
    problems.push('the manifest names another revision than the source commit');
  if (
    typeof manifest.revision_short !== 'string' ||
    !head.startsWith(manifest.revision_short) ||
    manifest.revision_short.length < 7
  )
    problems.push('the short revision is not the start of the commit');
  return problems;
}

/**
 * The fields of an `os-release` file (`/etc/os-release`), values unquoted.
 * @param {string} text
 * @returns {Record<string, string>}
 */
export function parseOsRelease(text) {
  /** @type {Record<string, string>} */
  const fields = {};
  for (const line of text.split('\n')) {
    const m = /^([A-Z0-9_]+)=(?:"([^"]*)"|'([^']*)'|(\S*))\s*$/.exec(line);
    if (m) fields[/** @type {string} */ (m[1])] = m[2] ?? m[3] ?? m[4] ?? '';
  }
  return fields;
}

/**
 * Whether a host is the one release artifacts are built in (`build-environment.json`).
 * @param {{ os?: { id?: unknown, version_id?: unknown } | null, architecture?: unknown } | null | undefined} host
 *   what the host is: its `os-release` ID and VERSION_ID, and `uname -m`
 * @param {{ os: { id: string, version_id: string }, architecture: string }} canonical
 * @returns {string[]} what differs; empty means it is that environment
 */
export function buildEnvironmentProblems(host, canonical) {
  const named = `${canonical.os.id} ${canonical.os.version_id} (${canonical.architecture})`;
  /** @type {string[]} */
  const problems = [];
  if (
    host?.os?.id !== canonical.os.id ||
    host?.os?.version_id !== canonical.os.version_id
  )
    problems.push(
      `the operating system is ${String(host?.os?.id ?? 'unknown')} ${String(host?.os?.version_id ?? '')}, not ${named}`.replace(
        /\s+,/,
        ',',
      ),
    );
  if (host?.architecture !== canonical.architecture)
    problems.push(
      `the architecture is ${String(host?.architecture ?? 'unknown')}, not ${canonical.architecture}`,
    );
  return problems;
}

/**
 * The private-key blocks in `bytes`, each from its BEGIN line to its END line, and how
 * many begin without ending within a few kilobytes. A block that does not end is not
 * one that can be recognised, so it counts against the file.
 * @param {Buffer} bytes
 * @returns {{ digests: string[], unterminated: number }}
 */
export function privateKeyBlocks(bytes) {
  const begin = Buffer.from('-----BEGIN ');
  /** @type {string[]} */
  const digests = [];
  let unterminated = 0;
  let at = bytes.indexOf(begin);
  while (at !== -1) {
    const head = bytes.toString('latin1', at, at + 60);
    const kind = /^-----BEGIN ((?:[A-Z]+ )*PRIVATE KEY)-----/.exec(head)?.[1];
    if (kind) {
      const end = Buffer.from(`-----END ${kind}-----`);
      const stop = bytes.indexOf(end, at);
      if (stop === -1 || stop - at > 8192) unterminated += 1;
      else digests.push(sha256Of(bytes.subarray(at, stop + end.length)));
    }
    at = bytes.indexOf(begin, at + begin.length);
  }
  return { digests, unterminated };
}

/**
 * Whether every private-key block of a file is one that was reviewed for that file. The
 * exception is bound to the content of each block, never to a path and a kind: a block
 * that is not listed, or one that never ends, is refused.
 * @param {string} path path of the file in the package
 * @param {Buffer} bytes
 * @param {{ libraries: { file_pattern: string, blocks: string[] }[] }} reviewed
 */
export function onlyReviewedKeyBlocks(path, bytes, reviewed) {
  const library = reviewed.libraries.find((l) =>
    new RegExp(l.file_pattern).test(path),
  );
  if (!library) return false;
  const { digests, unterminated } = privateKeyBlocks(bytes);
  const known = new Set(library.blocks);
  return (
    unterminated === 0 &&
    digests.length > 0 &&
    digests.every((d) => known.has(d))
  );
}

/**
 * Where the `.digest_md5` section of an AppImage runtime is: the 16 bytes appimagetool
 * fills with the MD5 of the image it built. Read from the ELF section headers of the
 * runtime itself, and refused (null) unless the runtime is a little-endian ELF64 that has
 * exactly one such section, of 16 bytes, inside the file.
 * @param {Buffer} runtime
 * @returns {{ offset: number, size: number } | null}
 */
export function runtimeDigestField(runtime) {
  const u16 = (/** @type {number} */ at) => runtime.readUInt16LE(at);
  const u32 = (/** @type {number} */ at) => runtime.readUInt32LE(at);
  const u64 = (/** @type {number} */ at) => {
    const value = runtime.readBigUInt64LE(at);
    return value <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(value) : -1;
  };
  // ELF64 (class 2), little endian (data 1), and a header that holds its own fields.
  if (
    runtime.length < 64 ||
    runtime.readUInt32BE(0) !== 0x7f454c46 ||
    runtime[4] !== 2 ||
    runtime[5] !== 1
  )
    return null;
  const tableAt = u64(0x28);
  const entrySize = u16(0x3a);
  const count = u16(0x3c);
  const namesIndex = u16(0x3e);
  if (tableAt < 0 || entrySize < 64 || count === 0 || namesIndex >= count)
    return null;
  if (tableAt + count * entrySize > runtime.length) return null;
  /** @param {number} index */
  const section = (index) => {
    const at = tableAt + index * entrySize;
    return { name: u32(at), offset: u64(at + 0x18), size: u64(at + 0x20) };
  };
  const names = section(namesIndex);
  if (
    names.offset < 0 ||
    names.size < 0 ||
    names.offset + names.size > runtime.length
  )
    return null;
  const nameOf = (/** @type {number} */ at) => {
    if (at >= names.size) return null;
    const from = names.offset + at;
    const end = runtime.indexOf(0, from);
    return end === -1 || end > names.offset + names.size
      ? null
      : runtime.toString('latin1', from, end);
  };
  /** @type {{ offset: number, size: number }[]} */
  const found = [];
  for (let i = 0; i < count; i += 1) {
    const candidate = section(i);
    if (nameOf(candidate.name) !== '.digest_md5') continue;
    if (
      candidate.offset < 0 ||
      candidate.size !== 16 ||
      candidate.offset + candidate.size > runtime.length
    )
      return null;
    found.push({ offset: candidate.offset, size: candidate.size });
  }
  return found.length === 1 ? found[0] : null;
}

/**
 * Whether `made` is `pinned` with, at most, its `.digest_md5` section changed: the 16
 * bytes appimagetool writes there (the MD5 of the image it was building). Where that
 * section is comes from the pinned runtime's own section headers; every byte outside it,
 * including those that touch it, has to be the pinned byte. A pinned runtime whose field
 * cannot be located pins nothing.
 * @param {Buffer} made
 * @param {Buffer} pinned
 */
export function runtimeIsPinned(made, pinned) {
  if (made.length !== pinned.length) return false;
  const field = runtimeDigestField(pinned);
  if (field === null) return false;
  const end = field.offset + field.size;
  return (
    made.subarray(0, field.offset).equals(pinned.subarray(0, field.offset)) &&
    made.subarray(end).equals(pinned.subarray(end))
  );
}
