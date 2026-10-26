/// <reference types="node" />
import { spawnSync } from 'node:child_process';
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  credentialFiles,
  environmentArtifacts,
  scanCheckout,
  secretKindsIn,
} from '../scripts/repo-scan-lib.mjs';

// Full-checkout scans and fixture Git subprocesses can exceed Vitest's default
// five seconds while Rust builds are running in parallel. Keep the scanner's
// assertions unchanged with a bounded test-file timeout that covers that load.
vi.setConfig({ testTimeout: 30_000 });

const root = join(__dirname, '..');
let scratch: string;
beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), 'wyss-repo-scan-'));
});
afterEach(() => rmSync(scratch, { recursive: true, force: true }));

// ---------------------------------------------------------------- what a credential looks like

// Secrets are assembled here, at run time, so that this file holds none of the shapes
// the scan looks for and the scan of the checkout does not stop on its own test.
const fake = {
  openai: ['sk', '-', 'a1B2c3D4'.repeat(5)].join(''),
  anthropic: ['sk', '-ant-', 'Zy9Xw8Vu7'.repeat(4)].join(''),
  github: ['gh', 'p_', 'A1b2C3d4E5'.repeat(4)].join(''),
  aws: ['AK', 'IA', '0123456789ABCDEF'].join(''),
  slack: ['xo', 'xb-', '1234567890-abcdefghij1234'].join(''),
  google: ['AI', 'za', 'Sy' + 'a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q'].join(''),
  jwt: [
    'ey',
    'JhbGciOiJIUzI1NiJ9',
    '.ey',
    'JzdWIiOiIxMjM0NTY3ODkwIn0',
    '.',
    'dBjftJeZ4CVPmB92K27uhbUJ',
  ].join(''),
  bearer: [
    'Authori',
    'zation: Bear',
    'er ',
    'abcdefghijklmnopqrstuvwxyz0123',
  ].join(''),
  login: [
    '"refresh',
    '_token": "',
    'abcdefghijklmnopqrstuvwxyz0123456789',
    '"',
  ].join(''),
  url: ['https', '://user:hunter22@', 'example.com/path'].join(''),
  key: [
    '-----BEGIN ',
    'RSA PRIVATE ',
    'KEY-----\n',
    'MIIEogIBAAKCAQEA6yCv+BLrRP/dMPBXJWK21c0aqxIX6JkODL4K+zlyEURt8/Wp\n',
    '-----END RSA PRIVATE KEY-----',
  ].join(''),
  npm: [
    '//registry.npmjs.org/:_auth',
    'Token=',
    'npm_0123456789abcdefghijklmnop',
  ].join(''),
};

it('finds a credential file by its name, and says nothing of its content', () => {
  const bad = [
    '.env',
    'usr/share/.env.local',
    'home/auth.json',
    'etc/credentials.json',
    'usr/share/ssl/server.pem',
    'usr/share/id_rsa',
    'usr/share/deploy.key',
    'var/state.sqlite3',
    'var/control.sqlite3-wal',
    'home/.netrc',
    '.git-credentials',
  ];
  expect(credentialFiles(bad).map((f) => f.path)).toEqual(bad);
  for (const finding of credentialFiles(bad))
    expect(Object.keys(finding).sort()).toEqual(['kind', 'path']);
  // What a repository normally tracks is not one. `.npmrc` is a settings file: a token in
  // it is found by its content.
  expect(
    credentialFiles([
      'README.md',
      '.npmrc',
      'src/main.ts',
      'src-tauri/Cargo.lock',
      'package-lock.json',
      'tests/fixtures/app-info-v1.json',
      'docs/keys.md',
    ]),
  ).toEqual([]);
});

it('finds each class of credential in text or in a binary, and prints none', () => {
  for (const [kind, secret] of Object.entries(fake)) {
    expect(
      secretKindsIn(Buffer.from(`before ${secret} after\n`, 'latin1')).length,
      `${kind} in text`,
    ).toBeGreaterThan(0);
    const found = secretKindsIn(
      Buffer.concat([
        Buffer.alloc(4096, 0x90),
        Buffer.from(secret, 'latin1'),
        Buffer.from([0, 0xff, 0xfe, 0]),
      ]),
    );
    expect(found.length, `${kind} in a binary`).toBeGreaterThan(0);
    // A finding is the name of a kind, never the bytes that matched.
    for (const name of found) expect(name).not.toContain(secret.slice(8, 24));
  }
});

it('does not mistake ordinary text for a credential', () => {
  const text = [
    'sk-short is not a key',
    'the task-force kept a risk-register',
    'https://example.com/docs and ssh://git@example.com/repo.git',
    'Authorization: handled elsewhere',
    '"access_token": ""',
    'AKIA is a prefix',
    '-----BEGIN CERTIFICATE-----',
    // The words a TLS library looks for, and the strings it prints to explain itself.
    '-----BEGIN PRIVATE KEY-----',
    '-----BEGIN RSA PRIVATE KEY-----\n-----END RSA PRIVATE KEY-----',
    's://%s:%d HOME %s%s.netrc ftp@example.com',
    'ftp://user:pass@host',
    'eyJ not a token',
    'ghp_ short',
  ].join('\n');
  expect(secretKindsIn(Buffer.from(text, 'latin1'))).toEqual([]);
  expect(secretKindsIn(Buffer.alloc(100_000, 0x90))).toEqual([]);
});

it('fails closed when it cannot look at all of a file, instead of calling it clean', () => {
  // Assembled here so that this file is not itself a credential-shaped string.
  const credential = [
    'registry http://deploy',
    ':hunter22secret',
    '@registry.example.com/\n',
  ].join('');
  // Found when it is the first thing in the file...
  expect(secretKindsIn(Buffer.from(credential))).toEqual([
    'credentials in a URL',
  ]);
  // ...and found behind any number of earlier places that look like the start of one.
  const buried = Buffer.from(
    'see https://example.com/x\n'.repeat(60_000) + credential,
  );
  expect(secretKindsIn(buried)).toEqual(['credentials in a URL']);
  // Past the most places it will look at, the file is reported, never passed.
  expect(secretKindsIn(buried, 100)).toEqual([
    'credentials in a URL (scan limit reached)',
  ]);
  expect(secretKindsIn(Buffer.from('nothing here'))).toEqual([]);
});

// ---------------------------------------------------------------- the scan of a checkout

/** Runs git in `repo` with an identity of its own. */
function gitIn(repo: string, ...args: string[]) {
  return spawnSync(
    'git',
    ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args],
    { cwd: repo, encoding: 'utf8' },
  );
}

/** A repository holding `files` (committed), scanned the way `npm run scan:repo` scans this one. */
function scanOf(
  files: Record<string, string | Buffer>,
  after: (repo: string) => void = () => {},
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
    [join(root, 'scripts/repo-scan.mjs'), '--repo-root', repo],
    { encoding: 'utf8', env: { PATH: process.env.PATH }, timeout: 60_000 },
  );
  return { status: result.status, out: result.stdout };
}

const HISTORICAL =
  'wyss-agent-legacy/.planning/phases/03-pipeline-de-ingesta/03-PATTERNS.md';
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
    [join(root, 'scripts/repo-scan.mjs')],
    { encoding: 'utf8', env: { PATH: process.env.PATH } },
  );
  expect(result.stdout).toMatch(
    /no credential in any of the \d+ tracked files/,
  );
  expect(result.stdout).not.toMatch(/^FAIL/m);
  expect(result.status).toBe(0);
  // There is no allowlist in the scan to be extended by a future change.
  for (const script of ['scripts/repo-scan.mjs', 'scripts/repo-scan-lib.mjs'])
    expect(readFileSync(join(root, script), 'utf8'), script).not.toMatch(
      /ALLOWED|03-PATTERNS|canary/,
    );
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

// A replace ref (`git replace`) makes Git answer for one object with another. What is
// scanned is the object a commit records, whatever stands in for it.
it('scans the objects a commit records, not the replacements a replace ref names', () => {
  const files = {
    'README.md': 'hello\n',
    'leak.txt': `token=${real.github}\n`,
  };
  // The whole commit replaced by a clean one, with the index and the disk made clean:
  // to Git the checkout is the clean commit, and `git status` says nothing.
  const hidden = (repo: string) => {
    const leaking = gitIn(repo, 'rev-parse', 'HEAD').stdout.trim();
    writeFileSync(join(repo, 'leak.txt'), 'clean\n');
    gitIn(repo, 'commit', '-q', '-a', '-m', 'clean');
    const clean = gitIn(repo, 'rev-parse', 'HEAD').stdout.trim();
    gitIn(repo, 'reset', '-q', '--soft', leaking);
    gitIn(repo, 'replace', '-f', leaking, clean);
    expect(gitIn(repo, 'rev-parse', 'HEAD').stdout.trim()).toBe(leaking);
    expect(gitIn(repo, 'status', '--porcelain').stdout).toBe('');
    expect(gitIn(repo, 'show', 'HEAD:leak.txt').stdout).toBe('clean\n');
  };
  // Only the committed file's object replaced by a clean one, and the disk made clean.
  const blob = (repo: string) => {
    const leaking = gitIn(repo, 'rev-parse', 'HEAD:leak.txt').stdout.trim();
    writeFileSync(join(repo, 'leak.txt'), 'clean\n');
    const clean = gitIn(repo, 'hash-object', '-w', 'leak.txt').stdout.trim();
    gitIn(repo, 'replace', '-f', leaking, clean);
    expect(gitIn(repo, 'show', 'HEAD:leak.txt').stdout).toBe('clean\n');
  };
  for (const [what, replace] of [
    ['the commit', hidden],
    ['the file', blob],
  ] as const) {
    const result = scanOf(files, replace);
    expect(result.status, what).toBe(1);
    expect(result.out, what).toMatch(
      /FAIL\s+no credential in any of the 2 tracked files: leak\.txt \(GitHub token\)/,
    );
    expect(result.out, what).not.toContain(real.github);
  }
  // A clean repository is clean, with or without a replace ref in it.
  expect(scanOf({ 'README.md': 'hello\n' }).status).toBe(0);
  const clean = scanOf({ 'README.md': 'hello\n' }, (repo) => {
    const first = gitIn(repo, 'rev-parse', 'HEAD').stdout.trim();
    gitIn(repo, 'commit', '-q', '--allow-empty', '-m', 'second');
    gitIn(repo, 'replace', '-f', first, 'HEAD');
  });
  expect(clean.out).not.toMatch(/^FAIL/m);
  expect(clean.status).toBe(0);
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
      [join(root, 'scripts/repo-scan.mjs'), '--repo-root', where],
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

// ------------------------------------------------ local-environment artifacts (2026-10-20)

/** Commits `files` plus links (`links`: path -> target) and reports the environment findings. */
function environmentOf(
  files: Record<string, string>,
  links: Record<string, string> = {},
) {
  const repo = join(scratch, `env-${Math.random().toString(16).slice(2)}`);
  mkdirSync(repo, { recursive: true });
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), content);
  }
  for (const [path, target] of Object.entries(links)) {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    symlinkSync(target, join(repo, path));
  }
  gitIn(repo, 'init', '-q');
  gitIn(repo, 'add', '-A');
  gitIn(repo, 'commit', '-q', '-m', 'files');
  return environmentArtifacts(repo, { personalNames: ['santi'] })
    .map((f) => `${f.path}: ${f.kind}`)
    .sort();
}

it('stops a dependency link to the working machine, as happened with node_modules', () => {
  expect(
    environmentOf(
      { 'a.txt': 'x' },
      { node_modules: '/home/santi/repo/node_modules' },
    ),
  ).toEqual([
    'node_modules: link to a path of the working machine',
    'node_modules: local dependency or build directory',
  ]);
});

it('stops links that leave the repository or point at a dependency directory', () => {
  expect(
    environmentOf({ 'a.txt': 'x' }, { 'x/out': '../../elsewhere' }),
  ).toEqual(['x/out: link that leaves the repository']);
  expect(
    environmentOf({ 'a.txt': 'x' }, { shortcut: 'node_modules/.bin/tsc' }),
  ).toEqual(['shortcut: link to a local dependency or build directory']);
});

it('stops dependency and build directories being tracked', () => {
  expect(
    environmentOf({
      'a/node_modules/m/index.js': '1',
      'src-tauri/target/x': '1',
      '.artifacts/r.json': '{}',
    }).sort(),
  ).toEqual([
    '.artifacts/r.json: local dependency or build directory',
    'a/node_modules/m/index.js: local dependency or build directory',
    'src-tauri/target/x: local dependency or build directory',
  ]);
});

it('stops the owner home directory in code and configuration but not in documentation or fixtures', () => {
  expect(
    environmentOf({
      'scripts/run.sh': 'cd /home/santi/Github_repos/x\n',
      'src/a.ts': "const p = '/Users/santi/x';",
      'docs/acceptance/a.md': 'ran in /home/santi/repo',
      'README.md': '/home/santi/.config',
      'tests/fixtures/a.json': '{"p":"/home/santi/x"}',
      'src/ok.ts':
        "const p = '/home/u/.codex'; // a fixture user, not the owner",
      'src/other.ts': "const p = '/home/santiago-x/';",
    }),
  ).toEqual([
    "scripts/run.sh: the owner's home directory named in code or configuration",
    "src/a.ts: the owner's home directory named in code or configuration",
  ]);
});

it('allows a relative link inside the repository and ordinary files', () => {
  expect(
    environmentOf(
      { 'docs/a.md': 'x', 'src/b.ts': 'y' },
      { 'docs/link.md': '../src/b.ts' },
    ),
  ).toEqual([]);
});
