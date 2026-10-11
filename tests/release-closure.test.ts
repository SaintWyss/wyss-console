/// <reference types="node" />
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, beforeEach, expect, it } from 'vitest';
import {
  checkFile,
  configProblems,
  directoryProblems,
  downloadsIn,
  offlineEnvironment,
  provision,
  provisionAll,
  sha256Of,
  transformed,
} from '../scripts/appimage-tools.mjs';
import {
  buildEnvironmentProblems,
  buildHostReferences,
  onlyReviewedKeyBlocks,
  parseOsRelease,
  privateKeyBlocks,
  revisionProblems,
  runtimeDigestField,
  runtimeIsPinned,
  scanCheckout,
  secretsIn,
  sourceProblems,
} from '../scripts/release-lib.mjs';

const root = join(__dirname, '..');
const committed = JSON.parse(
  readFileSync(join(root, 'scripts/appimage-tools.json'), 'utf8'),
);
let scratch: string;
beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), 'wyss-closure-'));
});
afterEach(() => rmSync(scratch, { recursive: true, force: true }));

// ---------------------------------------------------------------- the pinned tool table

it('pins every build tool the committed configuration names', () => {
  expect(configProblems(committed)).toEqual([]);
  expect(committed.tools.map((t: { name: string }) => t.name)).toEqual([
    'AppRun',
    'linuxdeploy',
    'linuxdeploy-plugin-appimage',
    'type2-runtime',
  ]);
  // The plugin would fetch its runtime from a floating address; it is handed the pinned one,
  // and the finished image has to begin with it.
  const packager = readFileSync(join(root, 'scripts/package.mjs'), 'utf8');
  expect(packager).toContain('LDAI_RUNTIME_FILE');
  expect(packager).toContain('does not begin with the pinned runtime');
  expect(
    readFileSync(join(root, 'scripts/release-verify.mjs'), 'utf8'),
  ).toContain('it begins with the pinned runtime');
  for (const tool of committed.tools) {
    // One fixed https release asset of github.com, an immutable tag, a digest for what is
    // downloaded and one for what runs, and a size bound.
    expect(tool.source).toMatch(
      /^https:\/\/github\.com\/[^/]+\/[^/]+\/releases\/download\/[^/]+\/[^/]+$/,
    );
    expect(tool.source).not.toMatch(
      /continuous|latest|nightly|\/HEAD|\/main\/|\/master\//i,
    );
    expect(tool.download_sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(tool.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(tool.max_bytes).toBeGreaterThan(0);
  }
});

it('names no floating reference anywhere the release is built from', () => {
  const sources = [
    'scripts/appimage-tools.json',
    'scripts/appimage-tools.mjs',
    'scripts/package.mjs',
    'src-tauri/tauri.conf.json',
  ].map((file) => [file, readFileSync(join(root, file), 'utf8')] as const);
  for (const [file, text] of sources) {
    // Code and data may talk about floating references to refuse them; they may not name
    // an address that is one.
    for (const url of text.match(/https?:\/\/[^\s"'`)]+/g) ?? [])
      expect(url, file).not.toMatch(
        /\/(continuous|latest|nightly|stable|HEAD|master|main)(\/|$)/i,
      );
  }
  // And the bundler is not told to fetch from anywhere else.
  const conf = readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8');
  expect(conf).not.toMatch(/linuxdeploy|appimagetool|AppRun/i);
});

it('refuses a configuration that could run something unpinned', () => {
  const tool = committed.tools[2];
  const broken = (change: Record<string, unknown>) =>
    configProblems({
      ...committed,
      tools: [
        { ...committed.tools[0], ...change },
        ...committed.tools.slice(1),
      ],
    });
  // The reference that was in use before: whatever is newest.
  expect(
    broken({
      version: 'continuous',
      source:
        'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage',
    }),
  ).not.toEqual([]);
  for (const floating of [
    'latest',
    'nightly',
    'stable',
    'HEAD',
    'main',
    'master',
  ])
    expect(
      broken({
        version: floating,
        source: `https://github.com/o/r/releases/download/${floating}/x.AppImage`,
      }),
      floating,
    ).not.toEqual([]);
  const cases: Record<string, Record<string, unknown>> = {
    'plain http': { source: tool.source.replace('https:', 'http:') },
    'another host': {
      source: 'https://example.com/o/r/releases/download/v1/x.AppImage',
    },
    'a branch, not a release': {
      source: 'https://github.com/o/r/raw/main/tool.AppImage',
    },
    'a query': { source: `${tool.source}?x=1` },
    'credentials in the address': {
      source: tool.source.replace('https://', 'https://user:pw@'),
    },
    'a version that is not the release': { version: 'v9' },
    'no digest': { download_sha256: '' },
    'a digest in capitals': { sha256: tool.sha256.toUpperCase() },
    'a short digest': { sha256: tool.sha256.slice(1) },
    'an unchanged file with two digests': {
      transform: null,
      sha256: '0'.repeat(64),
    },
    'an unknown transform': { transform: 'run-this' },
    'no size bound': { max_bytes: 0 },
    'an absurd size bound': { max_bytes: 10 ** 12 },
    'a path as a file name': { file: '../escape' },
    'no name': { name: '' },
  };
  for (const [what, change] of Object.entries(cases))
    expect(broken(change), what).not.toEqual([]);
  expect(configProblems(null)).not.toEqual([]);
  expect(
    configProblems({ schema_version: 2, tools: [], embedded: [] }),
  ).not.toEqual([]);
  expect(
    configProblems({ schema_version: 1, tools: [], embedded: [] }),
  ).not.toEqual([]);
  // Two tools cannot share a file in the cache.
  expect(
    configProblems({
      ...committed,
      tools: [
        committed.tools[0],
        { ...committed.tools[1], file: committed.tools[0].file },
      ],
    }),
  ).not.toEqual([]);
});

// ---------------------------------------------------------------- provisioning, offline

const body = Buffer.from('#!/bin/sh\necho a pinned tool\n');
/** A tool whose release is `release` and which runs as `runs`. */
function toolFor(release: Buffer, runs: Buffer = release, over = {}) {
  return {
    name: 'tool',
    file: 'tool.bin',
    version: '1.0',
    source: 'https://github.com/o/r/releases/download/1.0/tool.bin',
    download_sha256: sha256Of(release),
    sha256: sha256Of(runs),
    max_bytes: 1024,
    transform: null as null | 'clear-appimage-magic',
    ...over,
  };
}
const serving = (content: Buffer | (() => Buffer)) => {
  const calls: string[] = [];
  const fetch = (url: string, destination: string, max: number) => {
    calls.push(url);
    writeFileSync(
      destination,
      typeof content === 'function' ? content() : content,
    );
    void max;
  };
  return { fetch, calls };
};
const names = (directory: string) => readdirSync(directory).sort();

it('puts a tool in place only when what arrived is exactly the pinned release', () => {
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  const { fetch, calls } = serving(body);
  expect(provision(toolFor(body), dir, fetch)).toBe('fetched');
  expect(readFileSync(join(dir, 'tool.bin'))).toEqual(body);
  expect(lstatSync(join(dir, 'tool.bin')).mode & 0o777).toBe(0o755);
  expect(calls).toEqual([
    'https://github.com/o/r/releases/download/1.0/tool.bin',
  ]);
  // No temporary file is left beside it.
  expect(names(dir)).toEqual(['tool.bin']);
});

it('refuses a tool whose digest is not the pinned one, and leaves nothing in its place', () => {
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  const { fetch } = serving(Buffer.from('a different file'));
  expect(() => provision(toolFor(body), dir, fetch)).toThrow(
    /not the pinned release/,
  );
  expect(names(dir)).toEqual([]);
  // The same when the digest that is committed is the wrong one.
  const wrong = toolFor(body, body, {
    download_sha256: '0'.repeat(64),
    sha256: '0'.repeat(64),
  });
  expect(() => provision(wrong, dir, serving(body).fetch)).toThrow();
  expect(names(dir)).toEqual([]);
});

it('refuses a download that was cut short, or that is larger than its bound', () => {
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  expect(() =>
    provision(toolFor(body), dir, serving(body.subarray(0, 10)).fetch),
  ).toThrow(/not the pinned release/);
  expect(() =>
    provision(toolFor(body), dir, serving(Buffer.alloc(0)).fetch),
  ).toThrow();
  const big = Buffer.alloc(2048, 7);
  expect(() =>
    provision(toolFor(big, big, { max_bytes: 1024 }), dir, serving(big).fetch),
  ).toThrow(/too-large/);
  expect(names(dir)).toEqual([]);
});

it('never runs what a failed fetch left behind', () => {
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  const failing = (_u: string, destination: string) => {
    writeFileSync(destination, body.subarray(0, 5));
    throw new Error('connection reset');
  };
  expect(() => provision(toolFor(body), dir, failing)).toThrow(
    /connection reset/,
  );
  expect(names(dir)).toEqual([]);
  // No fetcher at all: a missing tool is a failure, not a silent skip.
  expect(() =>
    provision(toolFor(body), dir, () => {
      throw new Error('no network');
    }),
  ).toThrow(/no network/);
  expect(existsSync(join(dir, 'tool.bin'))).toBe(false);
});

it('hashes a cached tool every time, uses it only if exact, and replaces anything else', () => {
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  const tool = toolFor(body);
  provision(tool, dir, serving(body).fetch);
  // Exactly the pinned file: used, and nothing is fetched.
  const idle = serving(Buffer.from('must not be asked for'));
  expect(provision(tool, dir, idle.fetch)).toBe('cached');
  expect(idle.calls).toEqual([]);
  // The same name and the same size with one byte changed: not trusted.
  const tampered = Buffer.from(body);
  tampered[tampered.length - 2] ^= 0xff;
  writeFileSync(join(dir, 'tool.bin'), tampered);
  expect(checkFile(join(dir, 'tool.bin'), tool.sha256, 1024)).toBe('mismatch');
  const again = serving(body);
  expect(provision(tool, dir, again.fetch)).toBe('fetched');
  expect(again.calls).toHaveLength(1);
  expect(readFileSync(join(dir, 'tool.bin'))).toEqual(body);
  // A replaced cache that cannot be repaired stops the build and is gone.
  writeFileSync(join(dir, 'tool.bin'), tampered);
  expect(() => provision(tool, dir, serving(tampered).fetch)).toThrow();
  expect(existsSync(join(dir, 'tool.bin'))).toBe(false);
});

it('does not follow a link in the place of a tool, and does not write through it', () => {
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  const elsewhere = join(scratch, 'elsewhere');
  writeFileSync(elsewhere, body);
  symlinkSync(elsewhere, join(dir, 'tool.bin'));
  // The link points at the right bytes, and is still not the tool.
  expect(checkFile(join(dir, 'tool.bin'), sha256Of(body), 1024)).toBe(
    'not-a-file',
  );
  const guard = writeFileSync;
  void guard;
  expect(provision(toolFor(body), dir, serving(body).fetch)).toBe('fetched');
  expect(lstatSync(join(dir, 'tool.bin')).isSymbolicLink()).toBe(false);
  expect(readFileSync(elsewhere)).toEqual(body);
  // A directory where the tool goes is removed too.
  rmSync(join(dir, 'tool.bin'));
  mkdirSync(join(dir, 'tool.bin'));
  writeFileSync(join(dir, 'tool.bin', 'x'), 'x');
  expect(provision(toolFor(body), dir, serving(body).fetch)).toBe('fetched');
  expect(lstatSync(join(dir, 'tool.bin')).isFile()).toBe(true);
});

it('applies the one change the bundler makes before the file is hashed, and refuses anything else', () => {
  const image = Buffer.concat([
    Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1, 0]),
    Buffer.from([0x41, 0x49, 0x02]),
    Buffer.alloc(64, 9),
  ]);
  const runs = transformed('clear-appimage-magic', image);
  expect([...runs.subarray(8, 11)]).toEqual([0, 0, 0]);
  expect(runs.subarray(0, 8)).toEqual(image.subarray(0, 8));
  expect(runs.subarray(11)).toEqual(image.subarray(11));
  expect(image[8]).toBe(0x41); // the input is not changed
  const dir = join(scratch, 'tools');
  mkdirSync(dir);
  const tool = toolFor(image, runs, { transform: 'clear-appimage-magic' });
  expect(provision(tool, dir, serving(image).fetch)).toBe('fetched');
  expect(readFileSync(join(dir, 'tool.bin'))).toEqual(runs);
  // A release that is not an AppImage of that type is not changed into one.
  expect(() => transformed('clear-appimage-magic', body)).toThrow(
    /not an AppImage/,
  );
  // What would run has to be the pinned file, not merely something that was downloaded.
  const other = toolFor(image, image, { transform: 'clear-appimage-magic' });
  rmSync(join(dir, 'tool.bin'));
  expect(() => provision(other, dir, serving(image).fetch)).toThrow(
    /would run/,
  );
  expect(names(dir)).toEqual([]);
});

it('empties the tool directory of everything unpinned before a build, and finds a replaced tool after it', () => {
  const dir = join(scratch, 'tools');
  const tool = toolFor(body);
  const embedded = {
    name: 'script',
    file: 'plugin.sh',
    provider: 'cli',
    sha256: sha256Of('echo hi\n'),
  };
  const config = {
    schema_version: 1 as const,
    tools: [committed.tools[0]],
    embedded: [] as unknown[],
  };
  void config;
  // A directory of its own, with a stranger in it and a script a previous run wrote.
  mkdirSync(dir);
  writeFileSync(join(dir, 'stranger'), 'x');
  writeFileSync(join(dir, 'plugin.sh'), 'echo hi\n');
  mkdirSync(join(dir, 'a-directory'));
  const ours = {
    schema_version: 1 as const,
    tools: [
      {
        ...tool,
        source: committed.tools[0].source,
        version: committed.tools[0].version,
      },
    ],
    embedded: [embedded],
  };
  // The configuration of this test is not a real one, so only the tool matters here.
  const check = () => directoryProblems(ours, dir, 'before');
  expect(check().join(' ')).toMatch(/stranger/);
  for (const name of readdirSync(dir))
    rmSync(join(dir, name), { recursive: true, force: true });
  provision(tool, dir, serving(body).fetch);
  expect(check()).toEqual([]);
  // Before the bundler runs the scripts it writes are not expected; after, they are.
  expect(directoryProblems(ours, dir, 'after').join(' ')).toMatch(
    /script \(plugin\.sh\): missing/,
  );
  writeFileSync(join(dir, 'plugin.sh'), 'echo hi\n');
  expect(directoryProblems(ours, dir, 'after')).toEqual([]);
  // A tool replaced while the bundler ran, and a script it was handed, are caught.
  writeFileSync(join(dir, 'tool.bin'), 'replaced');
  expect(directoryProblems(ours, dir, 'after').join(' ')).toMatch(
    /tool \(tool\.bin\): mismatch/,
  );
  writeFileSync(join(dir, 'plugin.sh'), 'echo pwned\n');
  expect(directoryProblems(ours, dir, 'after').join(' ')).toMatch(
    /script \(plugin\.sh\): mismatch/,
  );
});

it('provisions the whole table from a fetcher that serves the pinned files, and no other', () => {
  const dir = join(scratch, 'cache', 'tauri');
  // Stand-ins whose digests are the table's: built from the table itself.
  const served = new Map<string, Buffer>();
  const config = {
    schema_version: 1 as const,
    tools: committed.tools.map((t: Record<string, unknown>, i: number) => {
      const content = Buffer.from(`release ${i}`);
      served.set(String(t.source), content);
      return {
        ...t,
        download_sha256: sha256Of(content),
        sha256: sha256Of(content),
        transform: null,
      };
    }),
    embedded: [],
  };
  const fetch = (url: string, destination: string) => {
    const content = served.get(url);
    if (!content) throw new Error(`unexpected address ${url}`);
    writeFileSync(destination, content);
  };
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'left-over'), 'x');
  const done = provisionAll(config, dir, fetch);
  expect(done.map((d) => d.how)).toEqual(committed.tools.map(() => 'fetched'));
  expect(names(dir)).toEqual(
    config.tools.map((t: { file: string }) => t.file).sort(),
  );
  expect(lstatSync(dir).mode & 0o777).toBe(0o700);
  // A second time every file is hashed and none is fetched.
  expect(
    provisionAll(config, dir, () => {
      throw new Error('no fetch');
    }).map((d) => d.how),
  ).toEqual(committed.tools.map(() => 'cached'));
  // An unpinned table is refused before anything is fetched.
  const floating = {
    ...config,
    tools: [{ ...config.tools[0], version: 'latest' }],
  };
  expect(() => provisionAll(floating, dir, fetch)).toThrow(/not pinned/);
});

it('stops a bundler that downloaded anything, and gives it no way out', () => {
  expect(
    downloadsIn(
      '   Compiling x\n Downloading https://example.com/tool\n    Finished\n',
    ),
  ).toEqual(['Downloading https://example.com/tool']);
  expect(downloadsIn('    Bundling x.AppImage\n    Finished 1 bundle')).toEqual(
    [],
  );
  const env = offlineEnvironment({
    HTTPS_PROXY: 'http://proxy:1',
    PATH: '/usr/bin',
    X: '1',
  });
  for (const name of [
    'HTTPS_PROXY',
    'https_proxy',
    'HTTP_PROXY',
    'http_proxy',
    'ALL_PROXY',
    'all_proxy',
  ])
    expect(env[name]).toBe('http://127.0.0.1:9');
  expect(env.CARGO_NET_OFFLINE).toBe('true');
  expect(env.NO_PROXY).toBe('');
  expect(env.PATH).toBe('/usr/bin');
});

// ---------------------------------------------------------------- the source gate

it('refuses a release of a tree that is not a commit', () => {
  const head = 'a'.repeat(40);
  expect(sourceProblems('', head)).toEqual([]);
  expect(sourceProblems('\n', head)).toEqual([]);
  expect(sourceProblems(' M src/main.rs\n', head).join(' ')).toMatch(
    /uncommitted changes/,
  );
  expect(sourceProblems('?? new-file\n', head).join(' ')).toMatch(
    /uncommitted changes/,
  );
  expect(sourceProblems(null, head).join(' ')).toMatch(/cannot be read/);
  expect(sourceProblems('', null).join(' ')).toMatch(/cannot be named/);
  expect(sourceProblems('', 'abc').join(' ')).toMatch(/cannot be named/);
});

it('refuses a manifest that names another commit than the source', () => {
  const head = 'b'.repeat(40);
  expect(
    revisionProblems(
      { revision: head, revision_short: head.slice(0, 12) },
      head,
    ),
  ).toEqual([]);
  expect(
    revisionProblems(
      { revision: 'c'.repeat(40), revision_short: 'c'.repeat(12) },
      head,
    ),
  ).not.toEqual([]);
  expect(
    revisionProblems({ revision: head, revision_short: 'c'.repeat(12) }, head),
  ).not.toEqual([]);
  expect(
    revisionProblems(
      { revision: head.slice(0, 12), revision_short: head.slice(0, 12) },
      head,
    ),
  ).not.toEqual([]);
  expect(
    revisionProblems({ revision: head, revision_short: 'bbb' }, head),
  ).not.toEqual([]);
  expect(revisionProblems({ revision: null }, head).length).toBeGreaterThan(0);
});

it('refuses to package a dirty tree, before it fetches or builds anything', () => {
  // A copy of the packaging scripts in a repository of their own, with an untracked file.
  const repo = join(scratch, 'repo');
  mkdirSync(join(repo, 'scripts'), { recursive: true });
  for (const file of readdirSync(join(root, 'scripts')))
    cpSync(join(root, 'scripts', file), join(repo, 'scripts', file));
  const git = (...args: string[]) =>
    spawnSync(
      'git',
      ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args],
      {
        cwd: repo,
        encoding: 'utf8',
      },
    );
  git('init', '-q');
  git('add', '-A');
  git('commit', '-q', '-m', 'scripts');
  const run = () =>
    spawnSync(
      process.execPath,
      [join(repo, 'scripts/package.mjs'), '--release'],
      {
        cwd: repo,
        encoding: 'utf8',
        // No way to fetch anything if it tried.
        env: {
          PATH: process.env.PATH,
          HOME: scratch,
          ...offlineEnvironment({}),
        },
      },
    );
  writeFileSync(join(repo, 'untracked.txt'), 'x');
  let result = run();
  expect(result.status).toBe(1);
  expect(result.stderr).toMatch(/uncommitted changes/);
  // A modified tracked file is just as dirty.
  rmSync(join(repo, 'untracked.txt'));
  writeFileSync(join(repo, 'scripts/release-lib.mjs'), '// edited\n', {
    flag: 'a',
  });
  result = run();
  expect(result.status).toBe(1);
  expect(result.stderr).toMatch(/uncommitted changes/);
  // Nothing was fetched or built: no cache, no artifacts.
  expect(existsSync(join(repo, '.cache'))).toBe(false);
  expect(existsSync(join(repo, '.artifacts'))).toBe(false);
  expect(existsSync(join(repo, 'src-tauri'))).toBe(false);
});

// ---------------------------------------------------------------- the secret scan

/** Runs git in `repo` with an identity of its own. */
function gitIn(repo: string, ...args: string[]) {
  return spawnSync(
    'git',
    ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args],
    { cwd: repo, encoding: 'utf8' },
  );
}

/** A repository holding `files` (committed), scanned the way `release:verify` scans the checkout. */
function scanOf(
  files: Record<string, string | Buffer>,
  after: (repo: string) => void = () => {},
  args: string[] = ['--repo-only'],
) {
  const repo = join(scratch, `repo-${Math.random().toString(16).slice(2)}`);
  mkdirSync(repo, { recursive: true });
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), content);
  }
  gitIn(repo, 'init', '-q');
  gitIn(repo, 'add', '-A');
  gitIn(repo, 'commit', '-q', '-m', 'files');
  after(repo);
  const result = spawnSync(
    process.execPath,
    [join(root, 'scripts/release-verify.mjs'), ...args, '--repo-root', repo],
    { encoding: 'utf8', env: { PATH: process.env.PATH }, timeout: 60_000 },
  );
  return { status: result.status, out: result.stdout };
}

const HISTORICAL =
  'docs origins/.planning/phases/03-pipeline-de-ingesta/03-PATTERNS.md';
// Built here, at run time, so that this file holds no credential-shaped string.
const real = {
  anthropic: [
    'sk',
    '-ant-',
    'api03-',
    'Zq8Rt2Lm5Xv7Bn3Kd9Wc4Hy6Jp1Fs0Ga',
    'AbCdEfGhIjKlMnOpQrStUvWxYz0123456789',
  ].join(''),
  openai: ['sk', '-proj-', 'Zq8Rt2Lm5Xv7Bn3Kd9Wc4Hy6Jp1Fs0GaAbCdEf'].join(''),
  github: ['gh', 'p_', 'Zq8Rt2Lm5Xv7Bn3Kd9Wc4Hy6Jp1Fs0GaAbCd'].join(''),
  aws: ['AK', 'IA', 'ZQ8RT2LM5XV7BN3K'].join(''),
  google: [
    'AI',
    'za',
    'SyZq8Rt2Lm5Xv7Bn3Kd9Wc4Hy6Jp1Fs0GaAbCdEfGh'.slice(0, 35),
  ].join(''),
};

it('the checkout is clean of credentials, with no exception to lean on', () => {
  const result = spawnSync(
    process.execPath,
    [join(root, 'scripts/release-verify.mjs'), '--repo-only'],
    { encoding: 'utf8', env: { PATH: process.env.PATH } },
  );
  expect(result.stdout).toMatch(
    /no credential in any of the \d+ tracked files/,
  );
  expect(result.stdout).not.toMatch(/^FAIL/m);
  expect(result.status).toBe(0);
  // There is no allowlist left in the verifier to be extended by a future change.
  const verifier = readFileSync(
    join(root, 'scripts/release-verify.mjs'),
    'utf8',
  );
  expect(verifier).not.toMatch(/ALLOWED|03-PATTERNS|canary/);
  // The planted string of the historical document was redacted, and only that.
  const historical = readFileSync(join(root, HISTORICAL), 'utf8');
  expect(historical).toContain('token=sk-ant-<FAKE_TEST_SECRET>');
  expect(historical).not.toMatch(/sk-ant-api03/);
});

it('stops a real-looking credential of any provider in the former canary path', () => {
  for (const [provider, secret] of Object.entries(real)) {
    const result = scanOf({
      [HISTORICAL]: `secret_file.write_text("token=${secret}")\n`,
    });
    expect(result.status, provider).toBe(1);
    expect(result.out, provider).toMatch(/FAIL\s+no credential in any of the/);
    expect(result.out, provider).toContain('03-PATTERNS.md');
    // The finding names the file and the kind and never the secret.
    expect(result.out, provider).not.toContain(secret);
    expect(result.out, provider).not.toContain(secret.slice(6, 30));
  }
});

it('stops the same credentials anywhere else, and a key block with a body', () => {
  for (const secret of Object.values(real))
    expect(scanOf({ 'docs/notes.md': `x ${secret} y\n` }).status).toBe(1);
  const pem = [
    '-----BEGIN ',
    'PRIVATE KEY-----\n',
    'MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgZq8Rt2Lm5Xv7Bn3K\n',
    '-----END PRIVATE KEY-----\n',
  ].join('');
  expect(scanOf({ 'docs/key.txt': pem }).status).toBe(1);
  expect(scanOf({ 'docs/x/.env': 'A=b\n' }).status).toBe(1);
});

it('accepts the redaction that replaced the canary, and ordinary prose about keys', () => {
  const result = scanOf({
    [HISTORICAL]: [
      'secret_file.write_text("token=sk-ant-<FAKE_TEST_SECRET>")',
      'ANTHROPIC_TEST_SECRET_REDACTED',
      'a key looks like sk-ant-... and starts with the letters sk',
      '-----BEGIN PRIVATE KEY----- is the first line of a key file',
    ].join('\n'),
  });
  expect(result.out).not.toMatch(/^FAIL/m);
  expect(result.status).toBe(0);
});

// A committed credential has to be found whatever the working tree of the checkout says:
// the scan reads what Git records, and a tracked path that is not on disk as Git records
// it is a failure of its own, never a file that is skipped.
it('finds a committed credential whose file is gone from the working tree', () => {
  const secret = real.github;
  const result = scanOf(
    { 'README.md': 'hello\n', 'docs/leak.txt': `token=${secret}\n` },
    (repo) => rmSync(join(repo, 'docs/leak.txt')),
  );
  expect(result.status).toBe(1);
  expect(result.out).toMatch(
    /FAIL\s+every tracked file was read.*docs\/leak\.txt \(tracked but missing from the working tree\)/,
  );
  expect(result.out).toMatch(
    /FAIL\s+no credential in any of the 2 tracked files: docs\/leak\.txt \(GitHub token\)/,
  );
  expect(result.out).not.toContain(secret);
});

it('finds a committed credential that the working tree replaced, hid or emptied', () => {
  const files = { 'README.md': 'hello\n', 'leak.txt': `token=${real.aws}\n` };
  const replacements: Record<string, (repo: string) => void> = {
    'clean content': (repo) => writeFileSync(join(repo, 'leak.txt'), 'clean\n'),
    'a directory': (repo) => {
      rmSync(join(repo, 'leak.txt'));
      mkdirSync(join(repo, 'leak.txt'));
    },
    'a link': (repo) => {
      rmSync(join(repo, 'leak.txt'));
      symlinkSync('README.md', join(repo, 'leak.txt'));
    },
    // Opening a FIFO would wait for a writer forever: it is judged, never read.
    'a fifo': (repo) => {
      rmSync(join(repo, 'leak.txt'));
      expect(spawnSync('mkfifo', [join(repo, 'leak.txt')]).status).toBe(0);
    },
    'a staged deletion': (repo) => gitIn(repo, 'rm', '-q', 'leak.txt'),
    'an untracked copy': (repo) =>
      gitIn(repo, 'rm', '-q', '--cached', 'leak.txt'),
  };
  for (const [what, replace] of Object.entries(replacements)) {
    const result = scanOf(files, replace);
    expect(result.status, what).toBe(1);
    expect(result.out, what).toMatch(
      /FAIL\s+no credential in any of the \d+ tracked files: leak\.txt \(AWS access key id\)/,
    );
  }
});

it('finds a credential that is only staged, and one that is only on disk', () => {
  // Staged, never committed, and then deleted from the disk: the index's blob is read.
  const staged = scanOf({ 'README.md': 'hello\n' }, (repo) => {
    writeFileSync(join(repo, 'new.txt'), `x ${real.openai}\n`);
    gitIn(repo, 'add', 'new.txt');
    rmSync(join(repo, 'new.txt'));
  });
  expect(staged.status).toBe(1);
  expect(staged.out).toMatch(/new\.txt \(OpenAI-style API key\)/);
  // Committed clean, changed on disk: the working tree is scanned too.
  const onDisk = scanOf({ 'README.md': 'hello\n' }, (repo) =>
    writeFileSync(join(repo, 'README.md'), `x ${real.openai}\n`),
  );
  expect(onDisk.status).toBe(1);
  expect(onDisk.out).toMatch(/README\.md \(OpenAI-style API key\)/);
});

it('scans what a tracked link points at on disk, without following it', () => {
  // Git stores a link as its target; changed on disk, the new target is scanned too.
  const result = scanOf({ 'README.md': 'hello\n' }, (repo) => {
    symlinkSync('README.md', join(repo, 'link'));
    gitIn(repo, 'add', 'link');
    gitIn(repo, 'commit', '-q', '-m', 'link');
    rmSync(join(repo, 'link'));
    symlinkSync(real.github, join(repo, 'link'));
  });
  expect(result.status).toBe(1);
  expect(result.out).toMatch(/link \(GitHub token\)/);
  expect(result.out).not.toContain(real.github);
  // A clean link is clean.
  const clean = scanOf({ 'README.md': 'hello\n' }, (repo) => {
    symlinkSync('README.md', join(repo, 'link'));
    gitIn(repo, 'add', 'link');
    gitIn(repo, 'commit', '-q', '-m', 'link');
  });
  expect(clean.out).not.toMatch(/^FAIL/m);
  expect(clean.status).toBe(0);
});

it('finds a credential inside committed binary content that is gone from the disk', () => {
  const binary = Buffer.concat([
    Buffer.from([0x7f, 0x45, 0x4c, 0x46, 0, 0, 0, 0xff, 0xfe]),
    Buffer.from(real.anthropic, 'latin1'),
    Buffer.from([0, 0, 0x90, 0x90, 0]),
  ]);
  const result = scanOf({ 'README.md': 'hi\n', 'bin/tool': binary }, (repo) =>
    rmSync(join(repo, 'bin/tool')),
  );
  expect(result.status).toBe(1);
  expect(result.out).toMatch(/bin\/tool \(Anthropic-style API key\)/);
});

it('fails closed when there is nothing it can vouch for', () => {
  // No commit to scan.
  const repo = join(scratch, 'uncommitted');
  mkdirSync(repo);
  writeFileSync(join(repo, 'a.txt'), 'a\n');
  gitIn(repo, 'init', '-q');
  gitIn(repo, 'add', '-A');
  const verify = (where: string) =>
    spawnSync(
      process.execPath,
      [
        join(root, 'scripts/release-verify.mjs'),
        '--repo-only',
        '--repo-root',
        where,
      ],
      { encoding: 'utf8', env: { PATH: process.env.PATH } },
    );
  let result = verify(repo);
  expect(result.status).toBe(1);
  expect(result.stdout).toMatch(/HEAD \(there is no commit to scan\)/);
  // Not a repository at all.
  const plain = join(scratch, 'plain');
  mkdirSync(plain);
  writeFileSync(join(plain, 'a.txt'), `x ${real.github}\n`);
  result = verify(plain);
  expect(result.status).toBe(1);
  // A submodule's content is not in this repository's objects: it is not scanned, so it
  // cannot pass.
  const sub = scanOf({ 'README.md': 'hi\n' }, (where) => {
    const head = gitIn(where, 'rev-parse', 'HEAD').stdout.trim();
    gitIn(
      where,
      'update-index',
      '--add',
      '--cacheinfo',
      `160000,${head},vendor/sub`,
    );
    mkdirSync(join(where, 'vendor/sub'), { recursive: true });
  });
  expect(sub.status).toBe(1);
  expect(sub.out).toMatch(
    /vendor\/sub \(a submodule, whose content is not scanned\)/,
  );
});

it('reports a committed file too large to scan, rather than passing it', () => {
  const repo = join(scratch, 'large');
  mkdirSync(repo);
  writeFileSync(join(repo, 'big.bin'), Buffer.alloc(4096, 0x61));
  writeFileSync(join(repo, 'small.txt'), 'small\n');
  gitIn(repo, 'init', '-q');
  gitIn(repo, 'add', '-A');
  gitIn(repo, 'commit', '-q', '-m', 'x');
  rmSync(join(repo, 'big.bin'));
  const scan = scanCheckout(repo, { maxBytes: 1024 });
  expect(scan.paths).toEqual(['big.bin', 'small.txt']);
  expect(scan.secrets).toEqual([
    { path: 'big.bin', kind: 'file too large to scan' },
  ]);
  expect(scan.problems).toEqual([
    { path: 'big.bin', problem: 'tracked but missing from the working tree' },
  ]);
  // The same checkout, complete and within the limit, is clean.
  writeFileSync(join(repo, 'big.bin'), Buffer.alloc(4096, 0x61));
  expect(scanCheckout(repo)).toEqual({
    paths: ['big.bin', 'small.txt'],
    secrets: [],
    problems: [],
  });
});

it('applies the same checkout scan in a full verification with --repo', () => {
  // An artifacts directory with nothing in it: the artifact checks fail on their own, and
  // the scan of the checkout still has to run and say what it found.
  const artifacts = join(scratch, 'artifacts');
  mkdirSync(artifacts);
  writeFileSync(join(artifacts, 'release-manifest.json'), '{}');
  writeFileSync(join(artifacts, 'SHA256SUMS'), '');
  const result = scanOf(
    { 'README.md': 'hello\n', 'leak.txt': `token=${real.github}\n` },
    (repo) => rmSync(join(repo, 'leak.txt')),
    ['--repo', '--artifacts', artifacts],
  );
  expect(result.status).toBe(1);
  expect(result.out).toMatch(/FAIL\s+every tracked file was read.*leak\.txt/);
  expect(result.out).toMatch(
    /FAIL\s+no credential in any of the 2 tracked files: leak\.txt \(GitHub token\)/,
  );
});

// ---------------------------------------------------------------- the build environment

const environment = JSON.parse(
  readFileSync(join(root, 'scripts/build-environment.json'), 'utf8'),
);

it('names one build environment, the one the reviewed library keys come from', () => {
  expect(environment.os).toEqual({ id: 'ubuntu', version_id: '24.04' });
  expect(environment.architecture).toBe('x86_64');
  expect(environment.recorded_packages).toContain('libgnutls30t64');
  const keys = JSON.parse(
    readFileSync(join(root, 'scripts/reviewed-library-keys.json'), 'utf8'),
  );
  for (const library of keys.libraries)
    expect(library.source).toMatch(/\(Ubuntu 24\.04\)$/);
  // The container that builds it starts from the same release, pinned by digest.
  const recipe = readFileSync(
    join(root, 'scripts/release.Containerfile'),
    'utf8',
  );
  expect(recipe).toMatch(
    /^FROM docker\.io\/library\/ubuntu:24\.04@sha256:[0-9a-f]{64}$/m,
  );
});

it('refuses every host but that one, and a host it cannot name', () => {
  const ubuntu = parseOsRelease(
    'PRETTY_NAME="Ubuntu 24.04.5 LTS"\nNAME="Ubuntu"\nVERSION_ID="24.04"\nID=ubuntu\nID_LIKE=debian\n',
  );
  expect(ubuntu.ID).toBe('ubuntu');
  expect(ubuntu.VERSION_ID).toBe('24.04');
  const host = (
    id: string | undefined,
    version: string | undefined,
    arch = 'x86_64',
  ) => ({
    os: { id, version_id: version },
    architecture: arch,
  });
  expect(
    buildEnvironmentProblems(host('ubuntu', '24.04'), environment),
  ).toEqual([]);
  const cachy = parseOsRelease(
    'NAME="CachyOS Linux"\nID=cachyos\nID_LIKE=arch\nBUILD_ID=rolling\n',
  );
  for (const [what, other] of [
    ['CachyOS', host(cachy.ID, cachy.VERSION_ID)],
    ['Arch', host('arch', undefined)],
    ['Ubuntu 22.04', host('ubuntu', '22.04')],
    ['Ubuntu 26.04', host('ubuntu', '26.04')],
    ['Debian 12', host('debian', '12')],
    ['an arm64 Ubuntu 24.04', host('ubuntu', '24.04', 'aarch64')],
    ['an unnamed host', host(undefined, undefined, '')],
  ] as const)
    expect(buildEnvironmentProblems(other, environment), what).not.toEqual([]);
  // A manifest that does not say where it was built is not one of that environment.
  expect(buildEnvironmentProblems(undefined, environment)).toHaveLength(2);
  expect(buildEnvironmentProblems(null, environment)).toHaveLength(2);
});

// ---------------------------------------------------------------- reviewed library keys

const reviewed = (blocks: string[]) => ({
  libraries: [
    { file_pattern: '^usr/lib/libgnutls\\.so(\\.[0-9.]+)?$', blocks },
  ],
});
const block = (body: string) =>
  `-----BEGIN PRIVATE KEY-----\n${body}\n-----END PRIVATE KEY-----`;

it('excuses a private key in a library only by the content of that exact key', () => {
  const known = block('AAAA-known-self-test-key');
  const lib = Buffer.from(`\0\0${known}\0\0`, 'latin1');
  const digest = sha256Of(Buffer.from(known));
  expect(privateKeyBlocks(lib).digests).toEqual([digest]);
  expect(
    onlyReviewedKeyBlocks('usr/lib/libgnutls.so.30', lib, reviewed([digest])),
  ).toBe(true);
  // Another block beside the known one: the file is no longer excused.
  const extra = Buffer.from(`${known}\0${block('BBBB-another-key')}`, 'latin1');
  expect(
    onlyReviewedKeyBlocks('usr/lib/libgnutls.so.30', extra, reviewed([digest])),
  ).toBe(false);
  // The same bytes in any other file, or under another library's name.
  expect(
    onlyReviewedKeyBlocks('usr/bin/wyss-control', lib, reviewed([digest])),
  ).toBe(false);
  expect(
    onlyReviewedKeyBlocks('usr/lib/libother.so.1', lib, reviewed([digest])),
  ).toBe(false);
  // A key that never ends cannot be recognised, so it is refused.
  const open = Buffer.from(
    `${known}\0-----BEGIN PRIVATE KEY-----\nCCCC`,
    'latin1',
  );
  expect(privateKeyBlocks(open).unterminated).toBe(1);
  expect(
    onlyReviewedKeyBlocks('usr/lib/libgnutls.so.30', open, reviewed([digest])),
  ).toBe(false);
  // A library with no key at all is not "reviewed" by an empty list.
  expect(
    onlyReviewedKeyBlocks(
      'usr/lib/libgnutls.so.30',
      Buffer.from('nothing'),
      reviewed([digest]),
    ),
  ).toBe(false);
  // The committed table lists digests, not keys.
  const file = JSON.parse(
    readFileSync(join(root, 'scripts/reviewed-library-keys.json'), 'utf8'),
  );
  for (const l of file.libraries)
    for (const d of l.blocks) expect(d).toMatch(/^[0-9a-f]{64}$/);
  chmodSync(scratch, 0o700);
});

/**
 * A small ELF64 little-endian file shaped like an AppImage runtime: code, then the
 * `.digest_md5` section (16 bytes) with `.upd_info` right behind it, then the section names
 * and the section header table.
 */
function elfRuntime(
  options: { sections?: string[]; digestSize?: number } = {},
) {
  const names = options.sections ?? ['.text', '.digest_md5', '.upd_info'];
  const sizes: Record<string, number> = {
    '.text': 2048,
    '.digest_md5': options.digestSize ?? 16,
    '.upd_info': 64,
  };
  const body: Record<string, number> = {};
  let at = 64;
  for (const name of names) {
    body[name] = at;
    at += sizes[name] ?? 32;
  }
  const strings = Buffer.from(`\0${['.shstrtab', ...names].join('\0')}\0`);
  const stringsAt = at;
  const tableAt = Math.ceil((stringsAt + strings.length) / 8) * 8;
  const count = names.length + 2;
  const file = Buffer.alloc(tableAt + count * 64, 0xaa);
  file.fill(0, 0, 64);
  file.writeUInt32BE(0x7f454c46, 0);
  file[4] = 2;
  file[5] = 1;
  file.writeBigUInt64LE(BigInt(tableAt), 0x28);
  file.writeUInt16LE(64, 0x3a);
  file.writeUInt16LE(count, 0x3c);
  file.writeUInt16LE(count - 1, 0x3e);
  strings.copy(file, stringsAt);
  file.fill(0, tableAt);
  const header = (
    index: number,
    nameAt: number,
    offset: number,
    size: number,
  ) => {
    const entry = tableAt + index * 64;
    file.writeUInt32LE(nameAt, entry);
    file.writeBigUInt64LE(BigInt(offset), entry + 0x18);
    file.writeBigUInt64LE(BigInt(size), entry + 0x20);
  };
  let nameAt = 1 + '.shstrtab'.length + 1;
  names.forEach((name, i) => {
    header(i + 1, nameAt, body[name], sizes[name] ?? 32);
    nameAt += name.length + 1;
  });
  header(count - 1, 1, stringsAt, strings.length);
  return { file, field: body['.digest_md5'], after: body['.upd_info'] };
}

it('takes a runtime for the pinned one only if it differs in nothing but its .digest_md5 section', () => {
  const { file: pinned, field, after } = elfRuntime();
  expect(runtimeIsPinned(Buffer.from(pinned), pinned)).toBe(true);
  // appimagetool writes the 16 bytes of the MD5 of the image into that one section.
  const made = Buffer.from(pinned);
  for (let i = 0; i < 16; i += 1) made[field + i] ^= 0xff;
  expect(runtimeIsPinned(made, pinned)).toBe(true);
  const one = Buffer.from(pinned);
  one[field + 7] ^= 1;
  expect(runtimeIsPinned(one, pinned)).toBe(true);
  // Nothing else, wherever it is: sixteen bytes of code are not the digest field.
  const code = Buffer.from(pinned);
  for (let i = 0; i < 16; i += 1) code[100 + i] ^= 0xff;
  expect(runtimeIsPinned(code, pinned)).toBe(false);
  // The byte on either side of the field, and a window that straddles its edge.
  const next = Buffer.from(pinned);
  next[after] ^= 1;
  expect(runtimeIsPinned(next, pinned)).toBe(false);
  const before = Buffer.from(pinned);
  before[field - 1] ^= 1;
  expect(runtimeIsPinned(before, pinned)).toBe(false);
  const straddle = Buffer.from(pinned);
  for (let i = 8; i < 24; i += 1) straddle[field + i] ^= 1;
  expect(runtimeIsPinned(straddle, pinned)).toBe(false);
  // The field changed and one more byte anywhere else.
  const both = Buffer.from(made);
  both[pinned.length - 1] ^= 1;
  expect(runtimeIsPinned(both, pinned)).toBe(false);
  // A different length is a different file.
  expect(runtimeIsPinned(pinned.subarray(1), pinned)).toBe(false);
  expect(
    runtimeIsPinned(Buffer.concat([pinned, Buffer.from([0])]), pinned),
  ).toBe(false);
});

it('pins nothing when the pinned runtime does not say where its digest field is', () => {
  expect(runtimeDigestField(elfRuntime().file)).toEqual({
    offset: elfRuntime().field,
    size: 16,
  });
  const refused = (pinned: Buffer) =>
    runtimeIsPinned(Buffer.from(pinned), pinned);
  // Not an ELF, or not an ELF64 little-endian one.
  expect(refused(Buffer.alloc(4096, 7))).toBe(false);
  const big = Buffer.from(elfRuntime().file);
  big[5] = 2;
  expect(refused(big)).toBe(false);
  const narrow = Buffer.from(elfRuntime().file);
  narrow[4] = 1;
  expect(refused(narrow)).toBe(false);
  // No such section, one of the wrong size, or the same name twice.
  expect(refused(elfRuntime({ sections: ['.text', '.upd_info'] }).file)).toBe(
    false,
  );
  expect(refused(elfRuntime({ digestSize: 15 }).file)).toBe(false);
  expect(
    refused(
      elfRuntime({ sections: ['.text', '.digest_md5', '.digest_md5'] }).file,
    ),
  ).toBe(false);
  // A header table that does not fit in the file, or a section that points outside it.
  const cut = elfRuntime().file;
  expect(refused(cut.subarray(0, cut.length - 1))).toBe(false);
  expect(refused(cut.subarray(0, 40))).toBe(false);
  const outside = Buffer.from(elfRuntime().file);
  const entry = outside.readUInt32LE(0x28) + 2 * 64;
  outside.writeBigUInt64LE(BigInt(outside.length - 8), entry + 0x18);
  expect(refused(outside)).toBe(false);
});

it('fails closed when it cannot read all of a file, instead of calling it clean', () => {
  // Assembled here so that this file is not itself a credential-shaped string.
  const credential = [
    'registry http://deploy',
    ':hunter22secret',
    '@registry.example.com/\n',
  ].join('');
  const entry = (name: string, text: string) => {
    const absolute = join(scratch, name);
    writeFileSync(absolute, text);
    return {
      type: 'file' as const,
      path: name,
      absolute,
      size: text.length,
      mode: 0o644,
    };
  };
  // Found when it is the first thing in the file...
  expect(secretsIn([entry('first.bin', credential)])).toEqual([
    { path: 'first.bin', kind: 'credentials in a URL' },
  ]);
  // ...and found behind any number of earlier places that look like the start of one.
  const filler = 'see https://example.com/x\n'.repeat(60_000);
  const buried = entry('buried.bin', filler + credential);
  expect(secretsIn([buried])).toEqual([
    { path: 'buried.bin', kind: 'credentials in a URL' },
  ]);
  // Past the most places it will look at, the file is reported, never passed.
  expect(secretsIn([buried], { maxHits: 100 })).toEqual([
    { path: 'buried.bin', kind: 'credentials in a URL (scan limit reached)' },
  ]);
  // A file too large to read is reported by both scans.
  expect(secretsIn([entry('huge.bin', 'x')], { maxBytes: 0 })).toEqual([
    { path: 'huge.bin', kind: 'file too large to scan' },
  ]);
  expect(
    buildHostReferences([entry('huge.bin', 'x')], ['/home/builder'], 0),
  ).toEqual([{ path: 'huge.bin', needle: '(file too large to scan)' }]);
  expect(secretsIn([entry('clean.bin', 'nothing here')])).toEqual([]);
});

it('finds the paths of any build machine, not only of the one verifying', () => {
  const program = (text: string) => {
    const path = join(scratch, `p-${Math.random().toString(16).slice(2)}`);
    writeFileSync(
      path,
      Buffer.concat([Buffer.from([0x7f, 0x45, 0]), Buffer.from(text)]),
    );
    return [
      {
        path: 'usr/bin/wyss-console',
        absolute: path,
        type: 'file' as const,
        mode: 0o755,
        size: text.length + 3,
      },
    ];
  };
  const kinds = (text: string) =>
    buildHostReferences(program(text), ['/somewhere/else']).map(
      (h) => h.needle,
    );
  expect(
    kinds('at /home/alice/src/wyss-console/src-tauri/src/main.rs'),
  ).toEqual(['a home directory']);
  expect(kinds('\0/root/.cache/x')).toEqual(["root's home directory"]);
  expect(kinds('/opt/ci/.cargo/registry/src/index/x.rs')).toEqual([
    'a Cargo home',
  ]);
  expect(kinds('/opt/ci/.rustup/toolchains/stable/lib')).toEqual([
    'a rustup toolchain',
  ]);
  expect(kinds('/ci/src-tauri/target/release/build/x-1/out')).toEqual([
    'a Cargo target directory',
  ]);
  expect(
    kinds('/ci/target/x86_64-unknown-linux-gnu/release/deps/x.rlib'),
  ).toEqual(['a Cargo target directory']);
  // What a release build writes instead, and what the toolchain itself carries, is fine.
  expect(
    kinds(
      '/build/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1/src/lib.rs\0' +
        '/rustc/2d8144b7/library/std/src/panicking.rs\0src/main.rs\0/usr/lib/x\0/proc/self/exe\0/chroot/x',
    ),
  ).toEqual([]);
});

it('does not say the bundler had no network, or that the product has no authority or agent UI', () => {
  const read = (file: string) => readFileSync(join(root, file), 'utf8');
  const documents = [
    'README.md',
    'docs/release/RELEASE.md',
    'docs/adr/0026-settings-and-release-closure.md',
    'scripts/package.mjs',
    'scripts/appimage-tools.mjs',
  ].map(read);
  for (const text of documents) {
    expect(text).not.toMatch(/bundler (then )?(runs|ran) with no network/i);
    expect(text).not.toMatch(/then bundles with no network/i);
    expect(text).not.toMatch(/no way out/i);
  }
  const readme = read('README.md');
  expect(readme).not.toContain('No agent SDK, authority or agent UI');
  expect(readme).not.toContain('Codex is not usable through the Console UI');
});

it('refuses to package with a tool table that names a floating tool, before it fetches anything', () => {
  const repo = join(scratch, 'repo-floating');
  mkdirSync(join(repo, 'scripts'), { recursive: true });
  for (const file of readdirSync(join(root, 'scripts')))
    cpSync(join(root, 'scripts', file), join(repo, 'scripts', file));
  // The table as it was before it was pinned: the plugin from the floating release.
  const table = JSON.parse(
    readFileSync(join(repo, 'scripts/appimage-tools.json'), 'utf8'),
  );
  table.tools[2].version = 'continuous';
  table.tools[2].source =
    'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage';
  writeFileSync(
    join(repo, 'scripts/appimage-tools.json'),
    JSON.stringify(table),
  );
  const git = (...args: string[]) =>
    spawnSync(
      'git',
      ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args],
      {
        cwd: repo,
        encoding: 'utf8',
      },
    );
  git('init', '-q');
  git('add', '-A');
  git('commit', '-q', '-m', 'scripts');
  const result = spawnSync(
    process.execPath,
    [join(repo, 'scripts/package.mjs'), '--release'],
    {
      cwd: repo,
      encoding: 'utf8',
      env: { PATH: process.env.PATH, HOME: scratch, ...offlineEnvironment({}) },
    },
  );
  expect(result.status).toBe(1);
  expect(result.stderr).toMatch(/names whatever is newest|floating release/);
  expect(existsSync(join(repo, '.cache'))).toBe(false);
  expect(existsSync(join(repo, 'src-tauri'))).toBe(false);
  expect(existsSync(join(repo, '.artifacts'))).toBe(false);
});
