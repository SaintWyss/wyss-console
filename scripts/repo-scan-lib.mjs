// The credential scan of a checkout, as functions that can be tested without one:
// `repo-scan.mjs` applies them to this repository and `tests/repo-scan.test.ts` applies
// them to repositories that were made on purpose to be wrong.
//
// Nothing here prints a secret: a finding names a file and a kind, never the bytes that
// matched.
import { spawnSync } from 'node:child_process';
import { lstatSync, readFileSync, readlinkSync } from 'node:fs';
import { join, posix } from 'node:path';

/**
 * Names that only a credential has, and that are therefore never tracked. `.npmrc` is not
 * one of them: it is a normal tracked file (this project's holds two settings), and what
 * could make it a credential is a token in it, which the content scan looks for.
 * @type {{ kind: string, test: RegExp }[]}
 */
export const CREDENTIAL_FILES = [
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
    test: /(^|\/)\.(netrc|pypirc|git-credentials)$/,
  },
  { kind: 'database', test: /\.(sqlite3?|db)(-wal|-shm)?$/i },
];

/**
 * @param {readonly string[]} paths
 * @returns {{ path: string, kind: string }[]}
 */
export function credentialFiles(paths) {
  /** @type {{ path: string, kind: string }[]} */
  const found = [];
  for (const path of paths)
    for (const { kind, test } of CREDENTIAL_FILES)
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
 * passed: a scanner that skips what it cannot read has certified nothing about it.
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
 * are matched, so a file of hundreds of megabytes takes seconds.
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
 * The environment of every Git command the scan takes an answer from: the objects Git
 * recorded, never a replacement for them. A replace ref (`git replace`) makes Git answer
 * for an object with another one, so a committed file reads as clean while the commit that
 * is named (and pushed) holds something else. Only these commands are given it; Git is not
 * changed for anything else.
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {NodeJS.ProcessEnv}
 */
export function recordedObjectsOnly(env = process.env) {
  return { ...env, GIT_NO_REPLACE_OBJECTS: '1' };
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
    env: recordedObjectsOnly(),
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
 * Directories that only a working machine has: dependencies, build output, local records.
 * Tracking one means a local artifact was staged (a `node_modules` link was, 2026-10-20).
 */
const LOCAL_DIRECTORIES = new Set([
  'node_modules',
  'target',
  '.artifacts',
  '.venv',
  '__pycache__',
]);

/**
 * Places where a path of the owner's own machine is documentation or history, not a dependency
 * of the build: prose, the changelog and the records of what was run where.
 * @param {string} path
 */
function mayNamePersonalPaths(path) {
  return (
    path.endsWith('.md') ||
    path.startsWith('docs/') ||
    path.startsWith('tests/fixtures/') ||
    path === 'scripts/repo-scan-lib.mjs' ||
    path === 'tests/repo-scan.test.ts'
  );
}

/**
 * Local-environment artifacts in what Git records: a link to the working machine, a dependency
 * or build directory, or code and configuration that names the owner's home directory. A
 * relative link inside the repository, documentation and fixtures are normal and not reported.
 * A finding names a path and a kind.
 * @param {string} where
 * @param {{ personalNames?: readonly string[] }} [options] account names whose home directory
 *   must not appear in code or configuration (default: the owner's, and the account running)
 * @returns {{ path: string, kind: string }[]}
 */
export function environmentArtifacts(where, options = {}) {
  const names = options.personalNames ?? [
    ...new Set(
      ['saintwyss', process.env.USER ?? ''].filter(
        (n) => n !== '' && n !== 'root',
      ),
    ),
  ];
  const home = names.length
    ? new RegExp(`(?:/home|/Users)/(?:${names.join('|')})(?:/|\\b)`)
    : null;
  const { found } = trackedRecords(where);
  /** @type {Map<string, { path: string, kind: string }>} */
  const out = new Map();
  /** @param {string} path @param {string} kind */
  const add = (path, kind) => out.set(`${path}\0${kind}`, { path, kind });
  const blobs = found.filter((r) => r.type === 'blob');
  const contents = blobContents(
    where,
    [...new Set(blobs.map((r) => r.oid))],
    MAX_SCAN,
  );
  for (const r of blobs) {
    const parts = r.path.split('/');
    if (parts.some((p) => LOCAL_DIRECTORIES.has(p)))
      add(r.path, 'local dependency or build directory');
    const bytes = contents.get(r.oid)?.bytes;
    if (bytes === undefined) continue;
    if (r.mode === '120000') {
      const target = bytes.toString('utf8');
      if (target.startsWith('/') || target.startsWith('~'))
        add(r.path, 'link to a path of the working machine');
      else if (
        posix
          .normalize(posix.join(posix.dirname(r.path), target))
          .startsWith('..')
      )
        add(r.path, 'link that leaves the repository');
      else if (target.split('/').some((p) => LOCAL_DIRECTORIES.has(p)))
        add(r.path, 'link to a local dependency or build directory');
      continue;
    }
    if (home !== null && !mayNamePersonalPaths(r.path) && !bytes.includes(0)) {
      if (home.test(bytes.toString('utf8')))
        add(
          r.path,
          "the owner's home directory named in code or configuration",
        );
    }
  }
  return [...out.values()];
}
