// Resolve only the package from the current repository's version and host architecture,
// then verify it against both its manifest and the current local release build.
// A leftover package from an older build must never be silently used in E2E.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { arch } from 'node:os';
import { isAbsolute, join, resolve } from 'node:path';

/** @param {string} path */
const sha256 = (path) =>
  createHash('sha256').update(readFileSync(path)).digest('hex');

/**
 * @param {string} repo absolute repository root
 * @param {string | undefined} specified optional WYSS_E2E_PACKAGE override
 * @returns {string} validated package root
 */
export function currentPackageDir(repo, specified) {
  const root = resolve(repo);
  const config = JSON.parse(
    readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'),
  );
  if (typeof config.version !== 'string' || !config.version) {
    throw new Error('invalid current desktop version');
  }
  const platform = `linux-${arch()}`;
  const expected = join(
    root,
    '.artifacts',
    'package',
    `wyss-console-${config.version}-${platform}`,
  );
  const directory = specified ? resolve(specified) : expected;
  if (!isAbsolute(directory) || !existsSync(directory)) {
    throw new Error('current desktop package does not exist; build it first');
  }
  if (
    !statSync(directory).isDirectory() ||
    realpathSync(directory) !== directory
  ) {
    throw new Error('desktop package directory is not canonical');
  }

  const manifestPath = join(
    directory,
    'share',
    'wyss-console',
    'package.json',
  );
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (
    manifest.schema_version !== 1 ||
    manifest.name !== 'wyss-console' ||
    manifest.version !== config.version ||
    manifest.platform !== platform
  ) {
    throw new Error('desktop package version or platform mismatch');
  }
  for (const [file, release] of [
    ['bin/wyss-console', 'wyss-console'],
    ['libexec/wyss-console/wyss-control', 'wyss-control'],
  ]) {
    const packagedPath = join(directory, file);
    const builtPath = join(root, 'src-tauri', 'target', 'release', release);
    if (
      !existsSync(packagedPath) ||
      !existsSync(builtPath) ||
      !statSync(packagedPath).isFile() ||
      realpathSync(packagedPath) !== packagedPath
    ) {
      throw new Error('desktop package file is missing or not canonical');
    }
    const digest = sha256(packagedPath);
    if (manifest.files?.[file] !== digest || sha256(builtPath) !== digest) {
      throw new Error('desktop package differs from current release build');
    }
    if (
      file === 'libexec/wyss-console/wyss-control' &&
      manifest.authority_sha256 !== digest
    ) {
      throw new Error('desktop package authority digest mismatch');
    }
  }
  return directory;
}
