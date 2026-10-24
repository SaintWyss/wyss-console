import { execFileSync } from 'node:child_process';

/** Freeze the exact clean source commit before either package build starts.
 * @param {string} root
 */
export function snapshotSource(root) {
  /** @param {...string} args */
  const git = (...args) =>
    execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  const revision = git('rev-parse', 'HEAD');
  if (!/^[0-9a-f]{40}$/.test(revision)) {
    throw new Error('package: invalid source revision');
  }
  if (git('status', '--porcelain', '--untracked-files=normal') !== '') {
    throw new Error('package: source tree must be clean');
  }
  return { revision };
}

/** @param {string} root @param {{ revision: string }} frozen */
export function assertSourceUnchanged(root, frozen) {
  if (snapshotSource(root).revision !== frozen.revision) {
    throw new Error('package: source revision changed during the build');
  }
}
