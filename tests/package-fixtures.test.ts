import { afterEach, describe, expect, it } from 'vitest';
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  FIXTURE_MARKERS,
  fixtureHitsInFile,
  verifyPackageFixtures,
} from '../scripts/package-fixtures.mjs';

const roots: string[] = [];
function root() {
  const directory = mkdtempSync(join(tmpdir(), 'wyss-package-fixtures-'));
  roots.push(directory);
  return directory;
}
function binary(directory: string, path: string, content: string | Buffer) {
  const target = join(directory, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, content);
  return target;
}
afterEach(() => {
  for (const directory of roots.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe('package fixture exclusion', () => {
  it('finds every known marker including one crossing a read boundary', () => {
    for (const marker of FIXTURE_MARKERS) {
      const content = Buffer.concat([
        Buffer.alloc(64 * 1024 - 3, 0xff),
        Buffer.from(marker),
        Buffer.alloc(12, 0),
      ]);
      const path = binary(root(), 'product', content);
      expect(fixtureHitsInFile(path)).toEqual([marker]);
    }
  });

  it('accepts only the four exact product files and catches contamination in any', () => {
    const directory = root();
    const desktop = binary(directory, 'bin/wyss-console', 'REAL-DESKTOP');
    const authority = binary(
      directory,
      'libexec/wyss-console/wyss-control',
      'REAL-AUTHORITY',
    );
    const parser = binary(
      directory,
      'libexec/wyss-console/wyss-screen',
      'REAL-PARSER',
    );
    const agent = binary(
      directory,
      'libexec/wyss-console/wyss-credential-agent',
      'REAL-AGENT',
    );
    expect(verifyPackageFixtures(directory)).toEqual([
      desktop,
      authority,
      parser,
      agent,
    ]);
    for (const path of [desktop, authority, parser, agent]) {
      writeFileSync(path, FIXTURE_MARKERS[0]);
      expect(() => verifyPackageFixtures(directory)).toThrow('Fixture backend');
      writeFileSync(path, 'REAL-BINARY');
    }
  });

  it('fails for absent, incomplete, empty, linked or nonregular payloads', () => {
    const directory = root();
    expect(() => verifyPackageFixtures(directory)).toThrow();
    const path = binary(directory, 'bin/wyss-console', 'REAL-DESKTOP');
    expect(() => verifyPackageFixtures(directory)).toThrow();
    writeFileSync(path, '');
    expect(() => fixtureHitsInFile(path)).toThrow('nonempty regular');
    const link = join(directory, 'linked');
    symlinkSync(path, link);
    expect(() => fixtureHitsInFile(link)).toThrow();
    expect(() => fixtureHitsInFile(directory)).toThrow('nonempty regular');
  });

  it('returns a failing CLI status for a missing package or contaminated binary', () => {
    const directory = root();
    const run = () =>
      spawnSync(
        process.execPath,
        ['scripts/check-no-fixture-backend.mjs', directory],
        { encoding: 'utf8' },
      );
    expect(run().status).toBe(1);
    binary(directory, 'bin/wyss-console', 'REAL-DESKTOP');
    binary(directory, 'libexec/wyss-console/wyss-screen', 'PARSER');
    const path = binary(
      directory,
      'libexec/wyss-console/wyss-control',
      'AUTHORITY',
    );
    binary(directory, 'libexec/wyss-console/wyss-credential-agent', 'AGENT');
    expect(run().status).toBe(0);
    writeFileSync(path, FIXTURE_MARKERS[1]);
    expect(run().status).toBe(1);
  });
});
