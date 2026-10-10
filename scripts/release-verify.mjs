// Opens the release artifacts that `npm run release:package` produced and checks, from
// the files themselves and nothing else, that they are what the manifest says and what
// the desktop needs: the right desktop, the authority it was pinned to in the place it
// looks for it, no credential, no development file, nothing tied to the machine that
// built them.
//
//   node scripts/release-verify.mjs [--artifacts <dir>] [--repo] [--source-root <dir>]
//   node scripts/release-verify.mjs --repo-only [--repo-root <dir>]
//
// `--repo` also scans the tracked files of the checkout for credentials, `--repo-only`
// does only that, and the manifest has to name the commit `--source-root` (the checkout by
// default) is at. Exit status 0
// only when every check passed. Nothing a check finds is printed as a value: a finding
// names a file and a kind.
import { spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { configProblems } from './appimage-tools.mjs';
import {
  buildHostReferences,
  escapingLinks,
  forbiddenPaths,
  onlyReviewedKeyBlocks,
  parseSums,
  payloadFor,
  permissionProblems,
  revisionProblems,
  secretsIn,
  sha256File,
  walk,
} from './release-lib.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

/** @type {string[]} */
const problems = [];
/** @type {string[]} */
const passed = [];
/**
 * @param {boolean} ok
 * @param {string} what
 * @param {string} [why]
 */
function check(ok, what, why = '') {
  if (ok) passed.push(what);
  else problems.push(why ? `${what}: ${why}` : what);
  return ok;
}

/**
 * @param {string} command
 * @param {string[]} args
 * @param {import('node:child_process').SpawnSyncOptionsWithStringEncoding} [options]
 */
function run(command, args, options = { encoding: 'utf8' }) {
  return spawnSync(command, args, {
    ...options,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
}

/** @param {string} tool */
function available(tool) {
  return run('sh', ['-c', `command -v ${tool}`]).status === 0;
}

const argv = process.argv.slice(2);
const flag = (/** @type {string} */ name) => argv.includes(name);
const option = (/** @type {string} */ name, /** @type {string} */ fallback) => {
  const at = argv.indexOf(name);
  return at !== -1 && argv[at + 1]
    ? /** @type {string} */ (argv[at + 1])
    : fallback;
};
const artifacts = resolve(
  option('--artifacts', join(root, '.artifacts', 'release')),
);

/** @param {string} path */
const json = (path) => JSON.parse(readFileSync(path, 'utf8'));
/** The checkout whose files are scanned, and the one whose commit the manifest must name. */
const repoRoot = resolve(option('--repo-root', root));
const sourceRoot = resolve(option('--source-root', root));
const reviewedKeys = json(join(root, 'scripts', 'reviewed-library-keys.json'));
const toolsConfig = json(join(root, 'scripts', 'appimage-tools.json'));
// Only the checkout: nothing about artifacts.
if (flag('--repo-only')) {
  scanRepo(repoRoot);
  finish();
}

// ---------------------------------------------------------------------------- versions
const versions = {
  package: json(join(root, 'package.json')).version,
  tauri: json(join(root, 'src-tauri', 'tauri.conf.json')).version,
  cargo: /^version\s*=\s*"([^"]+)"/m.exec(
    readFileSync(join(root, 'src-tauri', 'Cargo.toml'), 'utf8'),
  )?.[1],
};
check(
  versions.package === versions.tauri && versions.tauri === versions.cargo,
  'one version in package.json, tauri.conf.json and Cargo.toml',
  JSON.stringify(versions),
);

// ---------------------------------------------------------------------------- manifest
const manifestPath = join(artifacts, 'release-manifest.json');
if (!existsSync(manifestPath)) {
  console.error(`release-verify: no release manifest in ${artifacts}`);
  process.exit(1);
}
const manifest = json(manifestPath);
check(manifest.schema_version === 1, 'the manifest is schema version 1');
check(manifest.version === versions.package, 'the manifest names this version');
check(
  /^[0-9a-f]{64}$/.test(manifest.authority_sha256 ?? ''),
  'the manifest names one authority digest',
);
check(
  manifest.signing === 'unsigned',
  'the manifest does not claim a signature',
);
// The artifacts are of the commit that is being reviewed, and of nothing else.
const head = run('git', ['rev-parse', 'HEAD'], {
  cwd: sourceRoot,
  encoding: 'utf8',
});
const headSha = head.status === 0 ? head.stdout.trim() : '';
check(/^[0-9a-f]{40}$/.test(headSha), 'the source commit can be named');
const wrongRevision = revisionProblems(manifest, headSha);
check(
  wrongRevision.length === 0,
  'the manifest revision is the commit the source is at',
  wrongRevision.join('; '),
);
check(manifest.source_tree === 'clean', 'the manifest says the tree was clean');
check(
  manifest.network_during_bundling === 'none',
  'the manifest says the bundler ran with no network',
);
// The build tools: pinned in the repository, and the manifest says each was exactly that.
const unpinned = configProblems(toolsConfig);
check(
  unpinned.length === 0,
  'every AppImage build tool is pinned by version and digest',
  unpinned.join('; '),
);
for (const tool of toolsConfig.tools ?? []) {
  const said = (manifest.appimage_tooling?.downloaded ?? []).find(
    (/** @type {{ name: string }} */ t) => t.name === tool.name,
  );
  check(
    said?.version === tool.version &&
      said?.source === tool.source &&
      said?.download_sha256 === tool.download_sha256 &&
      said?.expected_sha256 === tool.sha256 &&
      said?.actual_sha256 === tool.sha256,
    `build tool ${tool.name} ${tool.version} was the pinned one when it ran`,
  );
}
for (const tool of toolsConfig.embedded ?? []) {
  const said = (manifest.appimage_tooling?.embedded ?? []).find(
    (/** @type {{ name: string }} */ t) => t.name === tool.name,
  );
  check(
    said?.expected_sha256 === tool.sha256 &&
      said?.actual_sha256 === tool.sha256,
    `build script ${tool.name} was the expected one when it ran`,
  );
}

// ---------------------------------------------------------------------------- checksums
const sums = parseSums(readFileSync(join(artifacts, 'SHA256SUMS'), 'utf8'));
/** @type {{ name: string, kind: string, size: number, sha256: string }[]} */
const listed = manifest.artifacts ?? [];
check(listed.length > 0, 'at least one installable artifact');
check(
  sums.size === listed.length && listed.every((a) => sums.has(a.name)),
  'SHA256SUMS lists exactly the artifacts of the manifest',
);
for (const artifact of listed) {
  const path = join(artifacts, artifact.name);
  const exists = existsSync(path);
  check(exists, `${artifact.name} exists`);
  if (!exists) continue;
  const actual = sha256File(path);
  check(
    actual === artifact.sha256,
    `${artifact.name} matches its manifest digest`,
  );
  check(
    actual === sums.get(artifact.name),
    `${artifact.name} matches SHA256SUMS`,
  );
  check(
    statSync(path).size === artifact.size,
    `${artifact.name} has the size it was recorded with`,
  );
}

// ---------------------------------------------------------------------------- the trees
const scratch = mkdtempSync(join(tmpdir(), 'wyss-release-'));
process.on('exit', () => rmSync(scratch, { recursive: true, force: true }));
const needles = [root, scratch].filter((p) => p.length > 5);
const pin = Buffer.from(manifest.authority_sha256 ?? '', 'ascii');

/**
 * What every installable package has to be, whichever format it came in. `tree` is the
 * package unpacked; `label` says which one.
 * @param {string} tree
 * @param {string} label
 */
function checkTree(tree, label) {
  const entries = walk(tree);
  const desktopPath = 'usr/bin/wyss-console';
  const authorityPath = payloadFor(desktopPath);
  const files = new Map(entries.map((e) => [e.path, e]));

  const desktop = files.get(desktopPath);
  const authority = files.get(authorityPath);
  check(desktop?.type === 'file', `${label}: the desktop is at ${desktopPath}`);
  check(
    authority?.type === 'file',
    `${label}: the authority is at ${authorityPath}, where the desktop looks for it`,
  );
  check(
    ((desktop?.mode ?? 0) & 0o111) !== 0 &&
      ((authority?.mode ?? 0) & 0o111) !== 0,
    `${label}: both programs are executable`,
  );
  if (desktop && authority) {
    check(
      sha256File(authority.absolute) === manifest.authority_sha256,
      `${label}: the authority is the one the manifest pinned`,
    );
    const desktopBytes = readFileSync(desktop.absolute);
    check(
      desktopBytes.includes(pin),
      `${label}: the desktop carries the digest of that authority`,
    );
    check(
      typeof manifest.revision_short === 'string' &&
        manifest.revision_short.length >= 7 &&
        desktopBytes.includes(Buffer.from(manifest.revision_short, 'ascii')),
      `${label}: the desktop was compiled with the revision the manifest names`,
    );
  }
  // One authority, one place: a second copy would be one nobody pinned.
  const authorities = entries.filter(
    (e) => e.type === 'file' && /(^|\/)wyss-control$/.test(e.path),
  );
  check(
    authorities.length === 1,
    `${label}: exactly one authority in the package`,
    authorities.map((e) => e.path).join(', '),
  );
  const desktops = entries.filter(
    (e) => e.type === 'file' && /(^|\/)wyss-console$/.test(e.path),
  );
  check(desktops.length === 1, `${label}: exactly one desktop in the package`);

  const forbidden = forbiddenPaths(entries);
  check(
    forbidden.length === 0,
    `${label}: no credential, fixture, source or local state by name`,
    forbidden.map((f) => `${f.path} (${f.kind})`).join(', '),
  );
  // The AppImage carries the build host's shared libraries, and GnuTLS compiles its own
  // published self-test keys into libgnutls. A private-key finding in that library is
  // dropped only when every block in it is one listed, by digest, in
  // `reviewed-library-keys.json`: the exception is bound to the content, and a block
  // that is not listed (or never ends) in that file or any other stops the release.
  const secrets = secretsIn(entries).filter(
    (s) =>
      !(
        s.kind === 'private key block' &&
        onlyReviewedKeyBlocks(
          s.path,
          readFileSync(join(tree, s.path)),
          reviewedKeys,
        )
      ),
  );
  check(
    secrets.length === 0,
    `${label}: no credential in any file`,
    secrets.map((s) => `${s.path} (${s.kind})`).join(', '),
  );
  const host = buildHostReferences(
    entries.filter((e) => e.path === desktopPath || e.path === authorityPath),
    needles,
  );
  check(
    host.length === 0,
    `${label}: neither program names the checkout or the build machine`,
    host.map((h) => h.path).join(', '),
  );
  const writable = permissionProblems(entries);
  check(
    writable.length === 0,
    `${label}: nothing is writable by anyone but its owner`,
    writable.map((w) => `${w.path} (${w.problem})`).join(', '),
  );
  const links = escapingLinks(entries);
  check(
    links.length === 0,
    `${label}: no link leaves the package`,
    links.map((l) => `${l.path} -> ${l.target}`).join(', '),
  );
  return entries;
}

for (const artifact of listed) {
  const path = join(artifacts, artifact.name);
  if (!existsSync(path)) continue;
  const tree = join(scratch, artifact.kind);
  mkdirSync(tree, { recursive: true });

  if (artifact.kind === 'deb') {
    if (!check(available('dpkg-deb'), 'dpkg-deb is available to open the .deb'))
      continue;
    const extracted = run('dpkg-deb', ['-x', path, tree]);
    if (
      !check(
        extracted.status === 0,
        `${artifact.name} unpacks`,
        extracted.stderr,
      )
    )
      continue;
    checkTree(tree, 'deb');

    // The package's own records.
    const control = run('dpkg-deb', ['-f', path]);
    const field = (/** @type {string} */ name) =>
      new RegExp(`^${name}: (.*)$`, 'm').exec(control.stdout)?.[1] ?? '';
    check(
      field('Package') === 'wyss-console',
      'deb: the package is named wyss-console',
    );
    check(
      field('Version') === versions.package,
      'deb: the package has the release version',
    );
    check(
      /\bsystemd\b/.test(field('Depends')),
      'deb: it depends on a service manager',
    );
    check(
      /libwebkit2gtk/.test(field('Depends')),
      'deb: it depends on the web view it uses',
    );
    check(
      /\bbubblewrap\b/.test(field('Recommends')),
      'deb: it recommends the sandbox Local Tools needs',
    );
    // Installing runs nothing: no script of the package's own, as root, at install time.
    const controlDir = join(scratch, 'deb-control');
    mkdirSync(controlDir, { recursive: true });
    run('dpkg-deb', ['-e', path, controlDir]);
    const scripts = walk(controlDir)
      .map((e) => e.path)
      .filter((name) => /^(pre|post)(inst|rm)$|^config$|^triggers$/.test(name));
    check(
      scripts.length === 0,
      'deb: it has no maintainer script',
      scripts.join(', '),
    );
    // Everything it installs is root's.
    const listing = run('dpkg-deb', ['-c', path])
      .stdout.split('\n')
      .filter(Boolean);
    const foreign = listing.filter((line) => !/^\S+\s+root\/root\s/.test(line));
    check(
      foreign.length === 0,
      'deb: every installed file belongs to root',
      `${foreign.length} entries`,
    );
    // The desktop entry the application menu uses.
    const desktopEntry = join(
      tree,
      'usr/share/applications/wyss-console.desktop',
    );
    const hasEntry = check(
      existsSync(desktopEntry),
      'deb: it installs an application menu entry',
    );
    if (hasEntry) {
      const text = readFileSync(desktopEntry, 'utf8');
      check(
        /^Type=Application$/m.test(text),
        'deb: the entry is an application',
      );
      check(
        /^Exec=wyss-console$/m.test(text),
        'deb: the entry starts the desktop by name',
      );
      check(
        /^Terminal=false$/m.test(text),
        'deb: the entry does not open a terminal',
      );
      check(
        /^Name=Wyss Console$/m.test(text),
        'deb: the entry carries the product name',
      );
      if (available('desktop-file-validate')) {
        const valid = run('desktop-file-validate', [desktopEntry]);
        check(
          valid.status === 0,
          'deb: the entry is a valid desktop entry',
          valid.stdout + valid.stderr,
        );
      }
    }
    check(
      existsSync(
        join(tree, 'usr/share/icons/hicolor/128x128/apps/wyss-console.png'),
      ),
      'deb: it installs an icon',
    );
  }

  if (artifact.kind === 'appimage') {
    // The image extracts itself: no FUSE and no display are needed to open it.
    const copy = join(scratch, 'image.AppImage');
    run('cp', [path, copy]);
    run('chmod', ['+x', copy]);
    const extracted = run(copy, ['--appimage-extract'], {
      cwd: tree,
      encoding: 'utf8',
      env: { PATH: process.env.PATH ?? '/usr/bin:/bin' },
    });
    if (
      !check(
        extracted.status === 0,
        `${artifact.name} unpacks`,
        extracted.stderr,
      )
    )
      continue;
    const image = join(tree, 'squashfs-root');
    check(existsSync(join(image, 'AppRun')), 'appimage: it has an entry point');
    checkTree(image, 'appimage');
    check(
      readdirSync(image).some((name) => name.endsWith('.desktop')),
      'appimage: it carries its menu entry',
    );
  }
}

// ---------------------------------------------------------------------------- the checkout
if (flag('--repo')) scanRepo(repoRoot);

finish();

/** Says what was checked and ends with the verdict. */
function finish() {
  for (const line of passed) console.log(`ok    ${line}`);
  for (const line of problems) console.log(`FAIL  ${line}`);
  console.log(
    `\nrelease-verify: ${passed.length} passed, ${problems.length} failed`,
  );
  process.exit(problems.length === 0 ? 0 : 1);
}

/**
 * The tracked files of a checkout: no credential file by name and no credential-shaped
 * content in any of them, historical documents included. There is no exception: a file
 * that holds something that looks like a credential is changed, not excused.
 * @param {string} where
 */
function scanRepo(where) {
  const listing = run('git', ['ls-files', '-z'], {
    cwd: where,
    encoding: 'utf8',
  });
  const names = listing.stdout.split('\0').filter(Boolean);
  const entries = names
    .map((path) => {
      const absolute = join(where, path);
      const stat = existsSync(absolute) ? statSync(absolute) : null;
      return stat?.isFile()
        ? /** @type {import('./release-lib.mjs').Entry} */ ({
            path,
            absolute,
            type: 'file',
            mode: stat.mode & 0o7777,
            size: stat.size,
          })
        : null;
    })
    .filter((e) => e !== null);
  check(entries.length > 0, 'the checkout has tracked files to scan');
  // Names that only a credential has. A fixture directory or a source file is a normal
  // thing to track; these are not. `.npmrc` is a normal tracked file (this project's
  // holds two settings); what could make it a credential is a token in it, which the
  // content scan below looks for.
  const tracked = forbiddenPaths(entries).filter(
    (f) =>
      !/(^|\/)\.npmrc$/.test(f.path) &&
      /environment|provider credentials|private key|package manager|database|credentials file/.test(
        f.kind,
      ),
  );
  check(
    tracked.length === 0,
    'no credential file is tracked',
    tracked.map((f) => `${f.path} (${f.kind})`).join(', '),
  );
  const secrets = secretsIn(entries);
  check(
    secrets.length === 0,
    `no credential in any of the ${entries.length} tracked files`,
    secrets.map((s) => `${s.path} (${s.kind})`).join(', '),
  );
}
