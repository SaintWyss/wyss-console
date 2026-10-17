import { spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
  NATIVE_STATE_MARKER,
  assertNativeState,
  assertPlainFileOrAbsent,
} from '../scripts/e2e-guard.mjs';
import { readLedger, recordProcess } from '../scripts/e2e-process-ledger.mjs';

const created: string[] = [];

function scratch(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  created.push(dir);
  return dir;
}

afterAll(() => {
  for (const path of created) rmSync(path, { recursive: true, force: true });
});

const root = '/tmp/wacc-ABCDEF';

function stateFor(overrides: Record<string, unknown> = {}) {
  return {
    harness: NATIVE_STATE_MARKER,
    root,
    pids: { systemd: 4100, xwayland: 4200 },
    groups: [4101, 4201],
    workspace: join(root, 'workspace'),
    display: ':77',
    ...overrides,
  };
}

describe('assertNativeState: fields used as cleanup targets', () => {
  it('accepts the state the harness writes', () => {
    expect(() => assertNativeState(stateFor(), root)).not.toThrow();
  });

  it('refuses a display that can leave the lock directory', () => {
    for (const display of [
      ':-x/../../etc',
      ':77/../../x',
      '77',
      ':',
      ':77-lock',
    ]) {
      expect(() => assertNativeState(stateFor({ display }), root)).toThrow(
        /invalid or foreign native E2E sandbox state/,
      );
    }
  });

  it('refuses process ids that are not positive integers', () => {
    for (const pids of [
      { xwayland: 0 },
      { xwayland: -1 },
      { xwayland: 1.5 },
      { xwayland: '4200' },
    ]) {
      expect(() => assertNativeState(stateFor({ pids }), root)).toThrow(
        /invalid or foreign/,
      );
    }
    expect(() => assertNativeState(stateFor({ groups: [0] }), root)).toThrow(
      /invalid or foreign/,
    );
  });

  it('refuses a state with the wrong marker, root or workspace', () => {
    expect(() =>
      assertNativeState(stateFor({ harness: 'other' }), root),
    ).toThrow();
    expect(() =>
      assertNativeState(stateFor({ root: '/tmp/wacc-ZZZZZZ' }), root),
    ).toThrow();
    expect(() =>
      assertNativeState(stateFor({ workspace: '/home/someone' }), root),
    ).toThrow();
    expect(() => assertNativeState(null, root)).toThrow();
  });
});

describe('assertPlainFileOrAbsent: state and ledger files are not links', () => {
  it('accepts a regular file and a missing file', () => {
    const dir = scratch('plain-');
    const file = join(dir, 'state.json');
    writeFileSync(file, '{}');
    expect(() => assertPlainFileOrAbsent(file)).not.toThrow();
    expect(() =>
      assertPlainFileOrAbsent(join(dir, 'absent.json')),
    ).not.toThrow();
  });

  it('refuses a symlink even when its destination holds valid content', () => {
    const dir = scratch('link-');
    const target = join(dir, 'elsewhere.json');
    writeFileSync(target, JSON.stringify({ harness: NATIVE_STATE_MARKER }));
    const link = join(dir, 'state.json');
    symlinkSync(target, link);
    expect(() => assertPlainFileOrAbsent(link)).toThrow(/not a regular file/);
  });
});

describe('down: a forged state cannot direct deletion outside the sandbox', () => {
  it('refuses a display path that escapes, and leaves the target file in place', () => {
    const victimDir = scratch('victim-');
    const victim = join(victimDir, 'victim-lock');
    writeFileSync(victim, 'keep');
    const tag = `t${process.pid}${Date.now() % 100000}`;
    const lockDir = `/tmp/.X-${tag}`;
    mkdirSync(lockDir);
    created.push(lockDir);
    const sandbox = mkdtempSync('/tmp/wacc-');
    created.push(sandbox);
    mkdirSync(join(sandbox, 'workspace'));
    writeFileSync(
      join(sandbox, 'state.json'),
      JSON.stringify(
        stateFor({
          root: sandbox,
          workspace: join(sandbox, 'workspace'),
          pids: { xwayland: 999999 },
          groups: [],
          display: `:-${tag}/../../..${victimDir}/victim`,
        }),
      ),
    );
    writeFileSync(
      join(sandbox, 'ledger.json'),
      JSON.stringify([{ name: 'x', pid: 999999, startTime: '1' }]),
    );
    const result = spawnSync(
      process.execPath,
      ['scripts/e2e-native.mjs', 'down', sandbox],
      { encoding: 'utf8' },
    );
    expect(result.status).not.toBe(0);
    expect(result.stderr).toMatch(
      /invalid or foreign native E2E sandbox state/,
    );
    expect(existsSync(victim)).toBe(true);
    expect(readFileSync(victim, 'utf8')).toBe('keep');
    expect(existsSync(sandbox)).toBe(true);
  });
});

describe('process ledger: a linked ledger is never read or written through', () => {
  it('refuses to read a ledger that is a symlink and leaves its target alone', () => {
    const dir = scratch('ledger-link-');
    const target = join(dir, 'their-ledger.json');
    const content = JSON.stringify([{ name: 'x', pid: 4300, startTime: '1' }]);
    writeFileSync(target, content);
    const sandbox = scratch('ledger-root-');
    symlinkSync(target, join(sandbox, 'ledger.json'));
    expect(() => readLedger(sandbox)).toThrow(/not a regular file/);
    expect(readFileSync(target, 'utf8')).toBe(content);
  });

  it('refuses to record a process through a linked ledger', () => {
    const dir = scratch('ledger-write-');
    const target = join(dir, 'their-file.json');
    writeFileSync(target, 'untouched');
    const sandbox = scratch('ledger-root-');
    symlinkSync(target, join(sandbox, 'ledger.json'));
    expect(() => recordProcess(sandbox, 'x', process.pid)).toThrow(
      /not a regular file/,
    );
    expect(readFileSync(target, 'utf8')).toBe('untouched');
  });
});
