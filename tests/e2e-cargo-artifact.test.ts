import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { seederFromCargoOutput } from '../scripts/e2e-cargo-artifact.mjs';

const root = mkdtempSync(join(tmpdir(), 'wyss-seeder-artifact-'));
const deps = join(root, 'target', 'debug', 'deps');
mkdirSync(deps, { recursive: true });
const current = join(deps, 'e2e_seed-012abc');
writeFileSync(current, 'new executable');
const stale = join(deps, 'e2e_seed-999aaa');
writeFileSync(stale, 'stale executable');

afterAll(() => rmSync(root, { recursive: true, force: true }));

const artifact = (
  executable: string,
  target = { name: 'e2e_seed', kind: ['test'] },
) => JSON.stringify({ reason: 'compiler-artifact', target, executable });
const finished = JSON.stringify({ reason: 'build-finished', success: true });
const output = (...records: string[]) => `${records.join('\n')}\n`;

describe('select the E2E seeder from this Cargo invocation', () => {
  it('chooses the reported current executable, never a stale directory peer', () => {
    expect(
      seederFromCargoOutput(output(artifact(current), finished), deps),
    ).toBe(current);
  });

  it('refuses a stale-only directory when the build reports no test artifact', () => {
    expect(() => seederFromCargoOutput(output(finished), deps)).toThrow(
      /exactly one current/,
    );
  });

  it('refuses multiple different matching executables', () => {
    expect(() =>
      seederFromCargoOutput(
        output(artifact(current), artifact(stale), finished),
        deps,
      ),
    ).toThrow(/exactly one current/);
  });

  it('ignores dependencies and unrelated test artifacts', () => {
    expect(
      seederFromCargoOutput(
        output(
          artifact(stale, { name: 'other_test', kind: ['test'] }),
          artifact(current),
          finished,
        ),
        deps,
      ),
    ).toBe(current);
  });

  it('requires a successful Cargo completion record', () => {
    expect(() =>
      seederFromCargoOutput(output(artifact(current)), deps),
    ).toThrow(/exactly one current/);
    expect(() =>
      seederFromCargoOutput(
        output(
          artifact(current),
          JSON.stringify({ reason: 'build-finished', success: false }),
        ),
        deps,
      ),
    ).toThrow(/did not finish successfully/);
  });

  it('rejects a reported executable outside Cargo deps', () => {
    const outside = join(root, 'e2e_seed-aaaa');
    writeFileSync(outside, 'external');
    expect(() =>
      seederFromCargoOutput(output(artifact(outside), finished), deps),
    ).toThrow(/unsafe/);
  });

  it('rejects a symlinked executable even inside Cargo deps', () => {
    const alias = join(deps, 'e2e_seed-aaccaa');
    symlinkSync(current, alias);
    expect(() =>
      seederFromCargoOutput(output(artifact(alias), finished), deps),
    ).toThrow(/unsafe/);
  });

  it('refuses malformed Cargo JSON', () => {
    expect(() =>
      seederFromCargoOutput(
        output('{invalid-json', artifact(current), finished),
        deps,
      ),
    ).toThrow(/invalid cargo JSON/);
  });
});
