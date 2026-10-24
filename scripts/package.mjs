// Builds the package a user runs: the desktop and the authority it was built with.
//
//   <out>/bin/wyss-console
//   <out>/libexec/wyss-console/wyss-control
//   <out>/libexec/wyss-console/wyss-screen
//   <out>/share/wyss-console/package.json
//
// The desktop installs and starts only the authority whose SHA-256 it was compiled
// with (`WYSS_AUTHORITY_SHA256`), and finds it beside itself. So the authority is built
// first, its digest is taken, and the desktop is built again knowing it. A build made
// any other way (`desktop:build`, `desktop:dev`) carries no authority and manages none.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { arch } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyPackageFixtures } from './package-fixtures.mjs';
import { snapshotSource, assertSourceUnchanged } from './build-source.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const release = join(root, 'src-tauri', 'target', 'release');
const tool = join(root, 'scripts', 'tool.mjs');
const source = snapshotSource(root);

/** @param {string} path */
const sha256 = (path) =>
  createHash('sha256').update(readFileSync(path)).digest('hex');

/** The same build `desktop:build` runs, with or without the authority's digest. */
/** @param {Record<string, string>} extra */
function build(extra) {
  const result = spawnSync(
    process.execPath,
    [tool, 'tauri', 'build', '--no-bundle', '--', '--locked'],
    {
      cwd: root,
      env: { ...process.env, ...extra, WYSS_BUILD_REVISION: source.revision },
      stdio: 'inherit',
    },
  );
  if (result.status !== 0) {
    console.error('package: the build failed');
    process.exit(result.status ?? 1);
  }
}

if (process.platform !== 'linux') {
  console.error('package: only Linux is packaged');
  process.exit(1);
}
// 1. Everything, once, without a pin: this is the authority that will be shipped.
const unpinned = { ...process.env };
delete unpinned.WYSS_AUTHORITY_SHA256;
process.env = unpinned;
build({});
const authority = join(release, 'wyss-control');
const digest = sha256(authority);
// 2. The desktop again, compiled with the digest of that authority.
build({ WYSS_AUTHORITY_SHA256: digest });
assertSourceUnchanged(root, source);
// The second build must not have produced another authority than the one it pinned.
if (sha256(authority) !== digest) {
  console.error('package: the authority changed while the desktop was built');
  process.exit(1);
}
// The terminal parser helper (M4): a small process of its own that keeps a terminal's screen. The
// host starts it from beside itself; it is part of what the manifest lists and the installer checks.
const screenHelper = join(release, 'wyss-screen');
const desktop = join(release, 'wyss-console');
if (!readFileSync(desktop).includes(Buffer.from(digest, 'ascii'))) {
  console.error('package: the desktop does not carry the authority digest');
  process.exit(1);
}

// 3. The layout, assembled beside its final place and moved there whole.
const { version } = JSON.parse(
  readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'),
);
const name = `wyss-console-${version}-linux-${arch()}`;
const parent = join(root, '.artifacts', 'package');
mkdirSync(parent, { recursive: true, mode: 0o755 });
const staging = mkdtempSync(join(parent, '.staging-'));
/** @type {[string, string][]} */
const files = [
  [desktop, 'bin/wyss-console'],
  [authority, 'libexec/wyss-console/wyss-control'],
  [screenHelper, 'libexec/wyss-console/wyss-screen'],
];
for (const [, target] of files) {
  mkdirSync(dirname(join(staging, target)), { recursive: true });
}
for (const [source, target] of files) {
  copyFileSync(source, join(staging, target));
  chmodSync(join(staging, target), 0o755);
}
mkdirSync(join(staging, 'share', 'wyss-console'), { recursive: true });
// What `install.sh` needs beside the binaries: itself, and the icon of the launcher entry.
copyFileSync(
  join(root, 'scripts', 'install.sh'),
  join(staging, 'share', 'wyss-console', 'install.sh'),
);
chmodSync(join(staging, 'share', 'wyss-console', 'install.sh'), 0o755);
copyFileSync(
  join(root, 'src-tauri', 'icons', '128x128.png'),
  join(staging, 'share', 'wyss-console', 'icon.png'),
);
writeFileSync(
  join(staging, 'share', 'wyss-console', 'package.json'),
  `${JSON.stringify(
    {
      schema_version: 1,
      name: 'wyss-console',
      version,
      source_revision: source.revision,
      platform: `linux-${arch()}`,
      authority_sha256: digest,
      files: Object.fromEntries(
        files.map(([, target]) => [target, sha256(join(staging, target))]),
      ),
    },
    null,
    2,
  )}\n`,
);
// Nobody but its owner writes anywhere in a package: the desktop refuses a payload
// that others could have replaced.
for (const directory of [
  staging,
  join(staging, 'bin'),
  join(staging, 'libexec'),
  join(staging, 'libexec', 'wyss-console'),
  join(staging, 'share'),
  join(staging, 'share', 'wyss-console'),
]) {
  chmodSync(directory, 0o755);
}
chmodSync(join(staging, 'share', 'wyss-console', 'package.json'), 0o644);
const out = join(parent, name);
// Refuse test fixtures before replacing the previous package artifact.
verifyPackageFixtures(staging);
rmSync(out, { recursive: true, force: true });
renameSync(staging, out);
console.log(`package: ${out}`);
console.log(`package: authority sha256 ${digest}`);
