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
  onlyReviewedKeyBlocks,
  privateKeyBlocks,
  revisionProblems,
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
  ).toEqual(['cached', 'cached', 'cached']);
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

/** A repository holding `files`, scanned the way `npm run release:verify` scans the checkout. */
function scanOf(files: Record<string, string>) {
  const repo = join(scratch, `repo-${Math.random().toString(16).slice(2)}`);
  mkdirSync(repo, { recursive: true });
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), content);
  }
  spawnSync('git', ['init', '-q'], { cwd: repo });
  spawnSync('git', ['add', '-A'], { cwd: repo });
  const result = spawnSync(
    process.execPath,
    [
      join(root, 'scripts/release-verify.mjs'),
      '--repo-only',
      '--repo-root',
      repo,
    ],
    { encoding: 'utf8', env: { PATH: process.env.PATH } },
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
