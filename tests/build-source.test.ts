/// <reference types="node" />
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import {
  snapshotSource,
  assertSourceUnchanged,
} from '../scripts/build-source.mjs';

it('refuses dirty sources and detects a commit moving between package build stages', () => {
  const root = mkdtempSync(join(tmpdir(), 'wyss-release-source-'));
  const git = (...args: string[]) =>
    execFileSync('git', args, { cwd: root, stdio: 'pipe' });
  const commit = () =>
    git(
      '-c',
      'user.name=Release fixture',
      '-c',
      'user.email=fixture@example.invalid',
      'commit',
      '--allow-empty',
      '-m',
      'fixture',
    );
  try {
    git('init', '--quiet');
    commit();
    const frozen = snapshotSource(root);
    expect(frozen.revision).toMatch(/^[0-9a-f]{40}$/);
    expect(() => assertSourceUnchanged(root, frozen)).not.toThrow();
    writeFileSync(join(root, 'changed.txt'), 'fixture');
    expect(() => snapshotSource(root)).toThrow('source tree must be clean');
    git('add', 'changed.txt');
    expect(() => snapshotSource(root)).toThrow('source tree must be clean');
    commit();
    expect(() => assertSourceUnchanged(root, frozen)).toThrow(
      'source revision changed',
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
