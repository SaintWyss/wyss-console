import { createHash } from 'node:crypto';
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { arch, tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { currentPackageDir } from '../scripts/e2e-package-select.mjs';

const roots: string[] = [];
const sha = (data: string) =>
  createHash('sha256').update(data).digest('hex');

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'e2e-package-'));
  roots.push(root);
  const configDir = join(root, 'src-tauri');
  const release = join(configDir, 'target', 'release');
  const dir = join(
    root,
    '.artifacts',
    'package',
    `wyss-console-0.2.0-linux-${arch()}`,
  );
  mkdirSync(release, { recursive: true });
  mkdirSync(join(dir, 'bin'), { recursive: true });
  mkdirSync(join(dir, 'libexec', 'wyss-console'), { recursive: true });
  mkdirSync(join(dir, 'share', 'wyss-console'), { recursive: true });
  writeFileSync(join(configDir, 'tauri.conf.json'), '{"version":"0.2.0"}');
  const desktop = 'desktop from this build';
  const authority = 'authority from this build';
  const files = {
    'bin/wyss-console': sha(desktop),
    'libexec/wyss-console/wyss-control': sha(authority),
  };
  writeFileSync(join(release, 'wyss-console'), desktop);
  writeFileSync(join(release, 'wyss-control'), authority);
  writeFileSync(join(dir, 'bin', 'wyss-console'), desktop);
  writeFileSync(
    join(dir, 'libexec', 'wyss-console', 'wyss-control'),
    authority,
  );
  const manifest = {
    schema_version: 1,
    name: 'wyss-console',
    version: '0.2.0',
    platform: `linux-${arch()}`,
    authority_sha256: sha(authority),
    files,
  };
  writeFileSync(
    join(dir, 'share', 'wyss-console', 'package.json'),
    JSON.stringify(manifest),
  );
  return { root, release, dir, manifest };
}

afterAll(() => {
  for (const root of roots) rmSync(root, { recursive: true, force: true });
});

describe('the current native package selector', () => {
  it('chooses the exact current version and architecture', () => {
    const { root, dir } = fixture();
    expect(currentPackageDir(root)).toBe(dir);
  });

  it('does not choose a stale package when another directory sorts first', () => {
    const { root, dir } = fixture();
    mkdirSync(
      join(root, '.artifacts', 'package', 'wyss-console-0.1.0-linux-x64'),
    );
    expect(currentPackageDir(root)).toBe(dir);
  });

  it('rejects a package whose release binary has changed', () => {
    const { root, release } = fixture();
    writeFileSync(join(release, 'wyss-control'), 'different release');
    expect(() => currentPackageDir(root)).toThrow(/differs/);
  });

  it('rejects a package with a mismatched or outdated platform', () => {
    const { root, dir, manifest } = fixture();
    writeFileSync(
      join(dir, 'share', 'wyss-console', 'package.json'),
      JSON.stringify({ ...manifest, platform: 'linux-other' }),
    );
    expect(() => currentPackageDir(root)).toThrow(/mismatch/);
  });

  it('refuses a stale override instead of silently launching it', () => {
    const { root, dir } = fixture();
    const stale = join(root, '.artifacts', 'package', 'not-current');
    mkdirSync(stale, { recursive: true });
    expect(() => currentPackageDir(root, stale)).toThrow();
    expect(currentPackageDir(root, dir)).toBe(dir);
  });
});
