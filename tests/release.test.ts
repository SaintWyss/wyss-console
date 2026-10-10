/// <reference types="node" />
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, beforeEach, expect, it } from 'vitest';
import {
  buildHostReferences,
  escapingLinks,
  forbiddenPaths,
  formatSums,
  parseSums,
  payloadFor,
  permissionProblems,
  secretsIn,
  sha256Of,
  walk,
} from '../scripts/release-lib.mjs';
import appInfo from './fixtures/app-info-v1.json';
import tauriConfig from '../src-tauri/tauri.conf.json';

const root = join(__dirname, '..');
let scratch: string;
beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), 'wyss-release-test-'));
});
afterEach(() => rmSync(scratch, { recursive: true, force: true }));

/** Writes `files` below the scratch directory and returns what `walk` sees. */
function tree(files: Record<string, string | Buffer>, mode = 0o644) {
  for (const [path, content] of Object.entries(files)) {
    const absolute = join(scratch, path);
    mkdirSync(dirname(absolute), { recursive: true });
    writeFileSync(absolute, content);
    chmodSync(absolute, mode);
  }
  return walk(scratch);
}

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
    'KEY-----\nMIIB\n-----END RSA PRIVATE KEY-----',
  ].join(''),
};

it('refuses a package by the name of what is in it', () => {
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
    '.git/config',
    '.cache/xdg-cache/x',
    'node_modules/pkg/index.js',
    'src-tauri/target/release/wyss-control',
    'tests/fixtures/authority-view-v1.json',
    'usr/share/fixtures/x',
    'src/main.test.ts',
    'tests/fixtures/codex_runtime.py',
    'src-tauri/src/main.rs',
    'Cargo.lock',
    'package-lock.json',
    '.npmrc',
    'acceptance-state/run-1',
  ];
  const found = forbiddenPaths(
    tree(Object.fromEntries(bad.map((path) => [path, 'x']))),
  ).map((f) => f.path);
  for (const path of bad) expect(found, path).toContain(path);
  // Every finding says what kind of thing it is, and nothing about its content.
  for (const finding of forbiddenPaths(tree({ '.env': 'SECRET_VALUE_123' })))
    expect(JSON.stringify(finding)).not.toContain('SECRET_VALUE_123');
});

it('accepts what a package is made of', () => {
  const good = tree({
    'usr/bin/wyss-console': 'desktop',
    'usr/libexec/wyss-console/wyss-control': 'authority',
    'usr/share/applications/wyss-console.desktop': '[Desktop Entry]',
    'usr/share/icons/hicolor/128x128/apps/wyss-console.png': 'png',
    AppRun: '#!/bin/sh',
    'wyss-console.desktop': '[Desktop Entry]',
  });
  expect(forbiddenPaths(good)).toEqual([]);
});

it('finds each class of credential in text or in a binary, and prints none', () => {
  for (const [kind, secret] of Object.entries(fake)) {
    const asText = tree({ [`${kind}.txt`]: `before ${secret} after\n` });
    expect(
      [...new Set(secretsIn(asText).map((s) => s.path))],
      `${kind} in text`,
    ).toEqual([`${kind}.txt`]);
    const asBinary = tree({
      [`${kind}.bin`]: Buffer.concat([
        Buffer.alloc(4096, 0x90),
        Buffer.from(secret, 'latin1'),
        Buffer.from([0, 0xff, 0xfe, 0]),
      ]),
    });
    expect(
      secretsIn(asBinary).some((s) => s.path === `${kind}.bin`),
      `${kind} in a binary`,
    ).toBe(true);
    for (const finding of secretsIn(asBinary)) {
      expect(Object.keys(finding).sort()).toEqual(['kind', 'path']);
      expect(JSON.stringify(finding)).not.toContain(secret.slice(8, 24));
    }
    rmSync(scratch, { recursive: true, force: true });
    mkdirSync(scratch);
  }
});

it('does not mistake ordinary text for a credential', () => {
  const entries = tree({
    'notes.txt': [
      'sk-short is not a key',
      'the task-force kept a risk-register',
      'https://example.com/docs and ssh://git@example.com/repo.git',
      'Authorization: handled elsewhere',
      '"access_token": ""',
      'AKIA is a prefix',
      '-----BEGIN CERTIFICATE-----',
      'eyJ not a token',
      'ghp_ short',
    ].join('\n'),
    'usr/bin/prog': Buffer.alloc(100_000, 0x90),
  });
  expect(secretsIn(entries)).toEqual([]);
});

it('finds a package that names the machine or the checkout that built it', () => {
  const entries = tree({
    'usr/bin/wyss-console': `panic at /home/builder/wyss-console/src/main.rs`,
    'usr/libexec/wyss-console/wyss-control': 'clean',
  });
  expect(buildHostReferences(entries, ['/home/builder/wyss-console'])).toEqual([
    { path: 'usr/bin/wyss-console', needle: '/home/builder/wyss-console' },
  ]);
  expect(buildHostReferences(entries, ['/home/other'])).toEqual([]);
});

it('finds what another account could have replaced', () => {
  const entries = [
    ...tree({ 'a/ok': 'x' }, 0o755),
    ...walk(scratch).filter(() => false),
  ];
  expect(permissionProblems(entries)).toEqual([]);
  for (const mode of [0o775, 0o757, 0o777, 0o666, 0o4755, 0o2755]) {
    rmSync(scratch, { recursive: true, force: true });
    mkdirSync(scratch);
    const found = permissionProblems(tree({ 'usr/bin/x': 'x' }, mode));
    expect(
      found.map((f) => f.path),
      mode.toString(8),
    ).toContain('usr/bin/x');
  }
});

it('refuses links that leave the package', () => {
  mkdirSync(join(scratch, 'usr/lib'), { recursive: true });
  writeFileSync(join(scratch, 'usr/lib/libx.so.1'), 'x');
  symlinkSync('libx.so.1', join(scratch, 'usr/lib/libx.so'));
  symlinkSync('../../../etc/passwd', join(scratch, 'usr/lib/out'));
  symlinkSync('/usr/bin/sh', join(scratch, 'usr/lib/abs'));
  expect(escapingLinks(walk(scratch)).map((l) => l.path)).toEqual([
    'usr/lib/abs',
    'usr/lib/out',
  ]);
});

// ---------------------------------------------------------------- layout and provenance

it('finds the authority below the prefix of the desktop, as the desktop does', () => {
  expect(payloadFor('usr/bin/wyss-console')).toBe(
    'usr/libexec/wyss-console/wyss-control',
  );
  expect(payloadFor('bin/wyss-console')).toBe(
    'libexec/wyss-console/wyss-control',
  );
  // The rule is the desktop's own: one `parent().parent()`, then `libexec/wyss-console/
  // wyss-control`. Kept in step by reading the code that applies it.
  const rust = readFileSync(
    join(root, 'src-tauri/src/lifecycle/package.rs'),
    'utf8',
  );
  expect(rust).toMatch(/parent\(\)\?\s*\.parent\(\)\?/);
  expect(rust).toMatch(
    /\.join\("libexec"\)\s*\.join\("wyss-console"\)\s*\.join\("wyss-control"\)/,
  );
});

it('configures the installers to put the authority exactly where the desktop looks', () => {
  const { linux } = tauriConfig.bundle;
  // deb: absolute destination, from the release build of the authority.
  expect(linux.deb.files).toEqual({
    [`/${payloadFor('usr/bin/wyss-console')}`]: 'target/release/wyss-control',
  });
  // AppImage: the same, below the AppDir.
  expect(linux.appimage.files).toEqual({
    [payloadFor('usr/bin/wyss-console')]: 'target/release/wyss-control',
  });
});

it('packages nothing but the two programs, with no script, no secret and no signing claim', () => {
  const bundle = tauriConfig.bundle as Record<string, unknown>;
  expect(bundle.targets).toEqual(['deb', 'appimage']);
  // No resource, sidecar or updater is bundled: the package is the desktop and the
  // authority, and nothing is updated behind the user's back.
  for (const key of [
    'resources',
    'externalBin',
    'createUpdaterArtifacts',
    'licenseFile',
    'fileAssociations',
  ])
    expect(bundle[key], key).toBeUndefined();
  const deb = tauriConfig.bundle.linux.deb as Record<string, unknown>;
  for (const key of [
    'preInstallScript',
    'postInstallScript',
    'preRemoveScript',
    'postRemoveScript',
    'desktopTemplate',
  ])
    expect(deb[key], key).toBeUndefined();
  // Signing is claimed nowhere in the configuration.
  expect(JSON.stringify(tauriConfig)).not.toMatch(/sign|pubkey|updater/i);
  expect(deb.depends).toContain('systemd');
  expect(deb.recommends).toContain('bubblewrap');
  // The window is granted exactly what it was: bundling added no capability.
  expect(
    (tauriConfig.app.security as { capabilities: string[] }).capabilities,
  ).toEqual(['main']);
});

it('has one version, everywhere it is written', () => {
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const cargo = /^version\s*=\s*"([^"]+)"/m.exec(
    readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8'),
  )?.[1];
  const lock = /name = "wyss-console"\nversion = "([^"]+)"/.exec(
    readFileSync(join(root, 'src-tauri/Cargo.lock'), 'utf8'),
  )?.[1];
  const npmLock = JSON.parse(
    readFileSync(join(root, 'package-lock.json'), 'utf8'),
  );
  expect(tauriConfig.version).toBe(pkg.version);
  expect(cargo).toBe(pkg.version);
  expect(lock).toBe(pkg.version);
  expect(npmLock.version).toBe(pkg.version);
  expect(npmLock.packages[''].version).toBe(pkg.version);
  expect(appInfo.version).toBe(pkg.version);
  // A release candidate of the 0.x line: it is not 1.0.
  expect(pkg.version).toMatch(/^0\.\d+\.\d+$/);
});

// ---------------------------------------------------------------- checksums

it('reads and writes checksums the way sha256sum does, and trusts nothing odd', () => {
  const a = sha256Of('one');
  const b = sha256Of('two');
  const text = `${a}  one.deb\n${b}  two.AppImage\n`;
  expect([...parseSums(text)]).toEqual([
    ['one.deb', a],
    ['two.AppImage', b],
  ]);
  expect(formatSums(parseSums(text))).toBe(text);
  for (const bad of [
    `${a}  one.deb\n${a}  one.deb\n`,
    `${a.toUpperCase()}  one.deb\n`,
    `${a.slice(1)}  one.deb\n`,
    `${a}  ../one.deb\n`,
    `${a}  dir/one.deb\n`,
    `${a} one.deb extra\n`,
    'not a checksum\n',
  ])
    expect(() => parseSums(bad), bad).toThrow();
  expect(sha256Of('abc')).toBe(
    'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
  );
});

// ---------------------------------------------------------------- the verifier, end to end

const dpkg = spawnSync('dpkg-deb', ['--version']).status === 0;

/** A package that is right, or wrong in the one way `wrong` says. */
function makeRelease(
  wrong:
    | 'none'
    | 'missing-authority'
    | 'other-authority'
    | 'desktop-without-pin'
    | 'credential-file'
    | 'secret-inside'
    | 'fixture'
    | 'script'
    | 'wrong-checksum'
    | 'signed-claim'
    | 'authority-by-path',
) {
  const authority = Buffer.from('the authority this release was pinned to');
  const pin = sha256Of(authority);
  const pkgRoot = join(scratch, 'pkg');
  const files: Record<string, string | Buffer> = {
    'usr/bin/wyss-console': Buffer.from(
      wrong === 'desktop-without-pin' ? 'desktop' : `desktop pinned to ${pin}`,
    ),
    'usr/libexec/wyss-console/wyss-control':
      wrong === 'other-authority'
        ? Buffer.from('another authority')
        : authority,
    'usr/share/applications/wyss-console.desktop':
      '[Desktop Entry]\nType=Application\nName=Wyss Console\nExec=wyss-console\nIcon=wyss-console\nTerminal=false\nCategories=Development;\n',
    'usr/share/icons/hicolor/128x128/apps/wyss-console.png': 'png',
  };
  if (wrong === 'missing-authority')
    delete files['usr/libexec/wyss-console/wyss-control'];
  if (wrong === 'credential-file')
    files['usr/share/wyss-console/auth.json'] = '{}';
  if (wrong === 'secret-inside')
    files['usr/share/wyss-console/notes.txt'] = `x ${fake.github} y`;
  if (wrong === 'fixture')
    files['usr/share/wyss-console/fixtures/a.json'] = '{}';
  if (wrong === 'authority-by-path')
    files['usr/share/wyss-console/path'] = '/usr/local/bin/wyss-control';
  for (const [path, content] of Object.entries(files)) {
    const absolute = join(pkgRoot, path);
    mkdirSync(dirname(absolute), { recursive: true });
    writeFileSync(absolute, content);
    chmodSync(
      absolute,
      path.startsWith('usr/bin') || path.includes('libexec') ? 0o755 : 0o644,
    );
  }
  const control = join(pkgRoot, 'DEBIAN');
  mkdirSync(control, { recursive: true });
  const version = JSON.parse(
    readFileSync(join(root, 'package.json'), 'utf8'),
  ).version;
  writeFileSync(
    join(control, 'control'),
    `Package: wyss-console\nVersion: ${version}\nArchitecture: amd64\nMaintainer: Wyss\nDescription: test\nDepends: systemd, libwebkit2gtk-4.1-0\nRecommends: bubblewrap\n`,
  );
  if (wrong === 'script') {
    writeFileSync(join(control, 'postinst'), '#!/bin/sh\nexit 0\n');
    chmodSync(join(control, 'postinst'), 0o755);
  }
  for (const directory of [
    pkgRoot,
    ...walk(pkgRoot)
      .filter((e) => e.type === 'dir')
      .map((e) => e.absolute),
  ])
    chmodSync(directory, 0o755);
  const out = join(scratch, 'release');
  mkdirSync(out);
  const name = `wyss-console_${version}_amd64.deb`;
  const built = spawnSync('dpkg-deb', [
    '--root-owner-group',
    '-b',
    pkgRoot,
    join(out, name),
  ]);
  if (built.status !== 0) throw new Error(String(built.stderr));
  const deb = readFileSync(join(out, name));
  writeFileSync(
    join(out, 'SHA256SUMS'),
    `${wrong === 'wrong-checksum' ? '0'.repeat(64) : sha256Of(deb)}  ${name}\n`,
  );
  writeFileSync(
    join(out, 'release-manifest.json'),
    JSON.stringify({
      schema_version: 1,
      name: 'wyss-console',
      version,
      revision: null,
      platform: 'linux-x86_64',
      authority_sha256: pin,
      artifacts: [
        { name, kind: 'deb', size: deb.length, sha256: sha256Of(deb) },
      ],
      signing: wrong === 'signed-claim' ? 'gpg' : 'unsigned',
      checksums: 'SHA256SUMS',
    }),
  );
  return out;
}
function verify(dir: string) {
  const result = spawnSync(
    process.execPath,
    [join(root, 'scripts/release-verify.mjs'), '--artifacts', dir],
    { encoding: 'utf8', env: { PATH: process.env.PATH } },
  );
  return {
    status: result.status,
    failures: result.stdout.split('\n').filter((l) => l.startsWith('FAIL')),
    out: result.stdout,
  };
}

it.skipIf(!dpkg)('accepts a package that is what its manifest says', () => {
  const result = verify(makeRelease('none'));
  expect(result.failures).toEqual([]);
  expect(result.status).toBe(0);
  expect(result.out).toContain(
    'the authority is at usr/libexec/wyss-console/wyss-control',
  );
});

it.skipIf(!dpkg)('refuses each way a package can be wrong', () => {
  const cases: [Parameters<typeof makeRelease>[0], RegExp][] = [
    ['missing-authority', /the authority is at usr\/libexec/],
    ['other-authority', /the authority is the one the manifest pinned/],
    ['desktop-without-pin', /the desktop carries the digest/],
    [
      'credential-file',
      /no credential, fixture, source or local state by name/,
    ],
    ['secret-inside', /no credential in any file/],
    ['fixture', /no credential, fixture, source or local state by name/],
    ['script', /it has no maintainer script/],
    ['wrong-checksum', /matches SHA256SUMS/],
    ['signed-claim', /does not claim a signature/],
  ];
  for (const [wrong, expected] of cases) {
    rmSync(scratch, { recursive: true, force: true });
    mkdirSync(scratch);
    const result = verify(makeRelease(wrong));
    expect(result.status, wrong).toBe(1);
    expect(result.failures.join('\n'), wrong).toMatch(expected);
    // Whatever it finds, it does not print the credential.
    expect(result.out, wrong).not.toContain(fake.github);
  }
});
