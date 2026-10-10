// Builds the package a user runs: the desktop and the authority it was built with.
//
//   <out>/bin/wyss-console
//   <out>/libexec/wyss-console/wyss-control
//   <out>/share/wyss-console/package.json
//
// With `--release` it also produces the installable artifacts, a `.deb` and an AppImage,
// in `.artifacts/release/`, with `SHA256SUMS` and `release-manifest.json`, and then
// opens them with `release-verify.mjs`. Both formats lay the two programs out below
// their prefix exactly as above (`usr/bin`, `usr/libexec/wyss-console`), so the desktop
// finds its authority in an installed package by the same rule it uses in this one.
//
// The desktop installs and starts only the authority whose SHA-256 it was compiled
// with (`WYSS_AUTHORITY_SHA256`), and finds it beside itself. So the authority is built
// first, its digest is taken, and the desktop is built again knowing it. A build made
// any other way (`desktop:build`, `desktop:dev`) carries no authority and manages none.
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { arch, release as kernelRelease } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  configProblems,
  curlFetch,
  describeTools,
  directoryProblems,
  downloadsIn,
  offlineEnvironment,
  provisionAll,
} from './appimage-tools.mjs';
import { sourceProblems } from './release-lib.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const release = join(root, 'src-tauri', 'target', 'release');
const tool = join(root, 'scripts', 'tool.mjs');

/** @param {string} path */
const sha256 = (path) =>
  createHash('sha256').update(readFileSync(path)).digest('hex');

const releasing = process.argv.includes('--release');

// What the binaries say about where they were built is the same wherever that was: the
// checkout and Cargo's sources are named by fixed, made-up paths. A package that names
// the machine that built it is a package that records it. The more specific prefix is
// given last, which is the one rustc prefers.
const remap = [
  `--remap-path-prefix=${root}=/build/wyss-console`,
  `--remap-path-prefix=${join(root, '.cache', 'cargo')}=/build/cargo`,
].join(' ');
process.env.RUSTFLAGS = [process.env.RUSTFLAGS, remap]
  .filter(Boolean)
  .join(' ');

/**
 * @param {string} command
 * @param {string[]} args
 * @param {import('node:child_process').SpawnSyncOptions} [options]
 */
function must(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  if (result.status !== 0) {
    console.error(`package: ${command} ${args[0] ?? ''} failed`);
    console.error(String(result.stderr ?? ''));
    process.exit(result.status ?? 1);
  }
  return String(result.stdout ?? '');
}

/**
 * Tauri bundles every binary of the crate into `usr/bin`, and the authority is one: a
 * second copy, on `PATH`, that no build pinned and nothing installs or starts. The only
 * authority a package carries is the one in `usr/libexec/wyss-console`, so the bundler's
 * copy is taken out, the menu entry gets a name a desktop can index, and the package is
 * written again, by `dpkg-deb`, with everything owned by root.
 * @param {string} source the bundler's .deb
 * @param {string} destination
 * @param {string} epoch seconds, so that the same sources give the same package
 */
function repackDeb(source, destination, epoch) {
  const tree = mkdtempSync(join(parent, '.deb-'));
  must('dpkg-deb', ['-R', source, tree]);
  rmSync(join(tree, 'usr', 'bin', 'wyss-control'), { force: true });
  const applications = join(tree, 'usr', 'share', 'applications');
  for (const name of readdirSync(applications))
    if (name.endsWith('.desktop') && name !== 'wyss-console.desktop')
      renameSync(
        join(applications, name),
        join(applications, 'wyss-console.desktop'),
      );
  // What the package says it contains is what it contains.
  /** @type {string[]} */
  const lines = [];
  let kilobytes = 0;
  /** @param {string} directory */
  const visit = (directory) => {
    for (const name of readdirSync(directory).sort()) {
      const path = join(directory, name);
      const relative = path.slice(tree.length + 1);
      if (relative === 'DEBIAN') continue;
      const stat = statSync(path);
      if (stat.isDirectory()) visit(path);
      else {
        lines.push(`${sha256Md5(path)}  ${relative}`);
        kilobytes += Math.ceil(stat.size / 1024);
      }
    }
  };
  visit(tree);
  writeFileSync(join(tree, 'DEBIAN', 'md5sums'), `${lines.join('\n')}\n`);
  const control = readFileSync(join(tree, 'DEBIAN', 'control'), 'utf8').replace(
    /^Installed-Size:.*$/m,
    `Installed-Size: ${kilobytes}`,
  );
  writeFileSync(join(tree, 'DEBIAN', 'control'), control);
  // xz: readable by every dpkg that is still in use, unlike the zstd the newest default to.
  must('dpkg-deb', ['--root-owner-group', '-Zxz', '-b', tree, destination], {
    env: { ...process.env, SOURCE_DATE_EPOCH: epoch },
  });
  rmSync(tree, { recursive: true, force: true });
}
/**
 * The same for the AppImage: the bundler's second copy of the authority is taken out of
 * its AppDir, and the image is written again, with the runtime the bundler chose and the
 * squashfs of the same kind, everything owned by root and every time the same.
 * @param {string} original the bundler's AppImage
 * @param {string} appDir the AppDir it was made from
 * @param {string} destination
 * @param {string} epoch seconds
 */
function repackAppImage(original, appDir, destination, epoch) {
  rmSync(join(appDir, 'usr', 'bin', 'wyss-control'), { force: true });
  // The runtime is everything in front of the filesystem.
  const offset = Number(must(original, ['--appimage-offset']).trim());
  if (!Number.isSafeInteger(offset) || offset <= 0) {
    console.error(
      'package: the AppImage does not say where its filesystem starts',
    );
    process.exit(1);
  }
  const runtime = readFileSync(original).subarray(0, offset);
  // The image begins with the pinned runtime and with nothing that was fetched.
  if (!runtimeFile || !runtime.equals(readFileSync(runtimeFile))) {
    console.error(
      'package: the AppImage does not begin with the pinned runtime',
    );
    process.exit(1);
  }
  const filesystem = join(parent, `.appimage-${process.pid}.squashfs`);
  rmSync(filesystem, { force: true });
  must('mksquashfs', [
    appDir,
    filesystem,
    '-root-owned',
    '-noappend',
    '-quiet',
    '-comp',
    'zstd',
    '-b',
    '131072',
    '-all-time',
    epoch,
    '-mkfs-time',
    epoch,
  ]);
  writeFileSync(
    destination,
    Buffer.concat([runtime, readFileSync(filesystem)]),
  );
  chmodSync(destination, 0o755);
  rmSync(filesystem, { force: true });
}

/** @param {string} path */
const sha256Md5 = (path) =>
  createHash('md5').update(readFileSync(path)).digest('hex');

/**
 * The same build `desktop:build` runs, with or without the authority's digest. With
 * `bundles` it also makes those installable formats from the build it just made, and
 * then it runs with no way out: the tools it needs are already in place and verified, so
 * a download it attempts has to fail, and one that is printed stops the release.
 * @param {Record<string, string>} extra
 * @param {string[]} [bundles]
 * @returns {Promise<string>} what the build printed
 */
async function build(extra, bundles = []) {
  const bundling = bundles.length > 0;
  const base = { ...process.env, ...extra };
  const child = spawn(
    process.execPath,
    [
      tool,
      'tauri',
      'build',
      ...(bundling ? ['--bundles', bundles.join(',')] : ['--no-bundle']),
      '--',
      '--locked',
    ],
    {
      cwd: root,
      env: bundling ? offlineEnvironment(base) : base,
      stdio: ['inherit', 'pipe', 'pipe'],
    },
  );
  let printed = '';
  /** @param {NodeJS.WriteStream} to */
  const tee = (to) => (/** @type {Buffer} */ chunk) => {
    printed += chunk.toString('utf8');
    to.write(chunk);
  };
  child.stdout.on('data', tee(process.stdout));
  child.stderr.on('data', tee(process.stderr));
  const status = await new Promise((resolve) => {
    child.on('error', () => resolve(1));
    child.on('exit', (code) => resolve(code ?? 1));
  });
  if (status !== 0) {
    console.error('package: the build failed');
    process.exit(Number(status) || 1);
  }
  return printed;
}

if (process.platform !== 'linux') {
  console.error('package: only Linux is packaged');
  process.exit(1);
}
/**
 * @param {string} command
 * @param {string[]} args
 */
function output(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8' });
  return result.status === 0 ? result.stdout.trim() : null;
}
/**
 * A tool of the build host: where it is, what it says it is, and its digest. Recorded so
 * that a build can be told from another; these come from the distribution, not from here.
 * @param {string} name
 */
function systemTool(name) {
  const located = output('sh', ['-c', `command -v ${name}`]);
  const real = located ? output('realpath', [located]) : null;
  const said =
    real &&
    (
      spawnSync(real, [name === 'mksquashfs' ? '-version' : '--version'], {
        encoding: 'utf8',
      }).stdout || ''
    )
      .split('\n')[0]
      ?.trim();
  return {
    name,
    path: real,
    version: said || null,
    sha256: real ? sha256(real) : null,
  };
}
// The source this is built from, named in the About section and in the manifest: the
// whole commit, and its first twelve digits that the desktop is compiled with.
const head = output('git', ['rev-parse', 'HEAD']);
const porcelain = output('git', ['status', '--porcelain']);
const revision = head === null ? null : head.slice(0, 12);
/** @type {Record<string, string>} */
const named =
  revision && /^[0-9a-f]{12}$/.test(revision)
    ? { WYSS_BUILD_REVISION: revision }
    : {};
// A release is the commit and nothing else, before anything is fetched or built.
if (releasing) {
  const wrong = sourceProblems(porcelain, head);
  if (wrong.length > 0) {
    for (const problem of wrong) console.error(`package: ${problem}`);
    process.exit(1);
  }
}

/** @type {import('./appimage-tools.mjs').ToolsConfig} */
const toolsConfig = JSON.parse(
  readFileSync(join(root, 'scripts', 'appimage-tools.json'), 'utf8'),
);
// Where the bundler looks for the tools it would otherwise download. `tool.mjs` gives it
// this XDG cache, so this is its `tauri` directory.
const toolsDirectory = join(root, '.cache', 'xdg-cache', 'tauri');
if (releasing) {
  const unpinned = configProblems(toolsConfig);
  if (unpinned.length > 0) {
    for (const problem of unpinned) console.error(`package: ${problem}`);
    process.exit(1);
  }
  // Every pinned tool is exactly what is pinned, or this stops. What was in the cache is
  // hashed, not believed; what the bundler writes itself is removed.
  try {
    for (const { name, how } of provisionAll(
      toolsConfig,
      toolsDirectory,
      curlFetch,
    ))
      console.log(`package: build tool ${name}: ${how}, sha256 verified`);
  } catch (error) {
    console.error(`package: ${error instanceof Error ? error.message : error}`);
    process.exit(1);
  }
}

// 1. Everything, once, without a pin: this is the authority that will be shipped.
const unpinned = { ...process.env };
delete unpinned.WYSS_AUTHORITY_SHA256;
process.env = unpinned;
await build(named);
const authority = join(release, 'wyss-control');
const digest = sha256(authority);
// 2. The desktop again, compiled with the digest of that authority. Bundled in the same
//    step when a release is asked for: the installers carry this desktop and this file.
if (releasing) {
  const before = directoryProblems(toolsConfig, toolsDirectory, 'before');
  if (before.length > 0) {
    for (const problem of before)
      console.error(`package: build tool ${problem}`);
    process.exit(1);
  }
}
// appimagetool, inside the plugin, takes the AppImage runtime from the file it is told
// of; without one it fetches a floating address. It is told of the pinned one.
const runtimeTool = toolsConfig.tools.find((t) => t.name === 'type2-runtime');
const runtimeFile = runtimeTool ? join(toolsDirectory, runtimeTool.file) : null;
if (releasing && !runtimeFile) {
  console.error('package: no pinned AppImage runtime is configured');
  process.exit(1);
}
const bundlerOutput = await build(
  {
    ...named,
    WYSS_AUTHORITY_SHA256: digest,
    ...(releasing && runtimeFile ? { LDAI_RUNTIME_FILE: runtimeFile } : {}),
  },
  releasing ? ['deb', 'appimage'] : [],
);
if (releasing) {
  // It had nothing to download. If it printed that it did, something unpinned ran.
  const downloads = downloadsIn(bundlerOutput);
  if (downloads.length > 0) {
    for (const line of downloads)
      console.error(`package: the bundler downloaded: ${line}`);
    process.exit(1);
  }
  // And what it used is what was verified, with the two scripts it wrote itself.
  const after = directoryProblems(toolsConfig, toolsDirectory, 'after');
  if (after.length > 0) {
    for (const problem of after)
      console.error(`package: build tool ${problem}`);
    process.exit(1);
  }
}
// The second build must not have produced another authority than the one it pinned.
if (sha256(authority) !== digest) {
  console.error('package: the authority changed while the desktop was built');
  process.exit(1);
}
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
];
for (const [, target] of files) {
  mkdirSync(dirname(join(staging, target)), { recursive: true });
}
for (const [source, target] of files) {
  copyFileSync(source, join(staging, target));
  chmodSync(join(staging, target), 0o755);
}
mkdirSync(join(staging, 'share', 'wyss-console'), { recursive: true });
writeFileSync(
  join(staging, 'share', 'wyss-console', 'package.json'),
  `${JSON.stringify(
    {
      schema_version: 1,
      name: 'wyss-console',
      version,
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
rmSync(out, { recursive: true, force: true });
renameSync(staging, out);
console.log(`package: ${out}`);
console.log(`package: authority sha256 ${digest}`);

if (releasing) {
  // 4. The installable artifacts, collected under clean names, with their checksums.
  const bundles = join(release, 'bundle');
  const target = join(root, '.artifacts', 'release');
  rmSync(target, { recursive: true, force: true });
  mkdirSync(target, { recursive: true, mode: 0o755 });
  const debArch =
    arch() === 'x64' ? 'amd64' : arch() === 'arm64' ? 'arm64' : arch();
  /** @type {{ from: string, name: string, kind: string }[]} */
  const wanted = [];
  for (const [kind, directory, extension] of /** @type {const} */ ([
    ['deb', 'deb', '.deb'],
    ['appimage', 'appimage', '.AppImage'],
  ])) {
    const found = existsSync(join(bundles, directory))
      ? readdirSync(join(bundles, directory)).filter((n) =>
          n.endsWith(extension),
        )
      : [];
    if (found.length !== 1) {
      console.error(
        `package: expected one ${kind} bundle, found ${found.length}`,
      );
      process.exit(1);
    }
    wanted.push({
      from: join(bundles, directory, /** @type {string} */ (found[0])),
      name: `wyss-console_${version}_${debArch}${extension}`,
      kind,
    });
  }
  // The time every file of the packages is given: the commit they are built from.
  const epoch = output('git', ['log', '-1', '--format=%ct']) ?? '0';
  const artifacts = wanted.map(({ from, name, kind }) => {
    const to = join(target, name);
    if (kind === 'deb') repackDeb(from, to, epoch);
    else
      repackAppImage(
        from,
        join(bundles, 'appimage', 'Wyss Console.AppDir'),
        to,
        epoch,
      );
    chmodSync(to, kind === 'appimage' ? 0o755 : 0o644);
    return { name, kind, size: statSync(to).size, sha256: sha256(to) };
  });
  writeFileSync(
    join(target, 'SHA256SUMS'),
    artifacts.map((a) => `${a.sha256}  ${a.name}\n`).join(''),
  );
  /** @param {string} command @param {string[]} args */
  const version_of = (command, args) =>
    output(command, args)?.split('\n')[0] ?? null;
  writeFileSync(
    join(target, 'release-manifest.json'),
    `${JSON.stringify(
      {
        schema_version: 1,
        name: 'wyss-console',
        version,
        revision: head,
        revision_short: revision,
        source_tree: 'clean',
        platform: `linux-${arch() === 'x64' ? 'x86_64' : arch() === 'arm64' ? 'aarch64' : arch()}`,
        authority_sha256: digest,
        artifacts,
        // Checksummed, not signed: there is no release signing key or process.
        signing: 'unsigned',
        checksums: 'SHA256SUMS',
        // Every executable the AppImage build ran that is not built here: where it came
        // from, the digest it had to have, and the digest it was found to have.
        appimage_tooling: describeTools(toolsConfig, toolsDirectory),
        // Run during bundling with no network (proxies unreachable, Cargo offline).
        network_during_bundling: 'none',
        // From the distribution's own packages, recorded and not pinned.
        system_tools: ['dpkg-deb', 'mksquashfs', 'patchelf', 'strip'].map(
          systemTool,
        ),
        toolchain: {
          rustc: version_of('rustc', ['--version']),
          cargo: version_of('cargo', ['--version']),
          node: process.version,
          npm: version_of('npm', ['--version']),
          tauri_cli: JSON.parse(
            readFileSync(join(root, 'package.json'), 'utf8'),
          ).devDependencies['@tauri-apps/cli'],
          kernel: kernelRelease(),
        },
      },
      null,
      2,
    )}\n`,
  );
  for (const a of artifacts)
    console.log(`package: ${a.name}  ${a.size} bytes  sha256 ${a.sha256}`);
  // The build changed nothing that is tracked and did not move HEAD: the artifacts are
  // of the commit they name.
  const after = sourceProblems(
    output('git', ['status', '--porcelain']),
    output('git', ['rev-parse', 'HEAD']),
  );
  if (after.length > 0 || output('git', ['rev-parse', 'HEAD']) !== head) {
    for (const problem of after) console.error(`package: ${problem}`);
    console.error('package: the source changed while the release was built');
    process.exit(1);
  }
  // 5. And opened again, from the files, before anyone is told it is a release.
  const verified = spawnSync(
    process.execPath,
    [join(root, 'scripts', 'release-verify.mjs'), '--repo'],
    {
      cwd: root,
      stdio: 'inherit',
    },
  );
  if (verified.status !== 0) process.exit(verified.status ?? 1);
}
