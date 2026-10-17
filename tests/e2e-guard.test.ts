import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
  FIXTURE_MARKERS,
  assertManagedSandbox,
  assertSandboxRoot,
  fixtureHits,
  fixtureHitsInFile,
  installFixtureCodex,
  productBinaries,
  sandboxEnv,
} from '../scripts/e2e-guard.mjs';

const real = { home: '/home/someone', uid: 1000 };
const fixturePath = resolve('src-tauri/tests/fixtures/codex_runtime.py');
const packageDir = resolve('.artifacts/package');

describe('assertSandboxRoot', () => {
  it('accepts a path outside the real home and the real user runtime', () => {
    expect(() =>
      assertSandboxRoot('/tmp/claude-1000/wacc-abc123', real),
    ).not.toThrow();
  });

  it('refuses a sandbox inside the real home', () => {
    expect(() => assertSandboxRoot('/home/someone/.codex/x', real)).toThrow(
      /real home/,
    );
    expect(() => assertSandboxRoot('/home/someone', real)).toThrow(/real home/);
  });

  it('refuses a sandbox inside the real user runtime', () => {
    expect(() => assertSandboxRoot('/run/user/1000/wacc', real)).toThrow(
      /runtime/,
    );
  });

  it('does not treat a sibling that shares a prefix as inside the home', () => {
    expect(() =>
      assertSandboxRoot('/home/someone-other/x', real),
    ).not.toThrow();
  });

  it('refuses a relative path', () => {
    expect(() => assertSandboxRoot('wacc-abc', real)).toThrow(/absolute/);
  });

  it('refuses a root whose control socket path would exceed the socket limit', () => {
    expect(() => assertSandboxRoot(`/tmp/${'x'.repeat(120)}`, real)).toThrow(
      /108/,
    );
  });
});

describe('assertManagedSandbox: destructive command boundary', () => {
  const host = { home: homedir(), uid: process.getuid() };

  it('accepts only an owned private sandbox created under /tmp/wacc-*', () => {
    const root = mkdtempSync('/tmp/wacc-');
    try {
      expect(() => assertManagedSandbox(root, host)).not.toThrow();
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('refuses an unrelated private directory, even outside HOME', () => {
    const root = mkdtempSync(join(tmpdir(), 'unrelated-e2e-'));
    try {
      expect(() => assertManagedSandbox(root, host)).toThrow(/canonical/);
      // Executing the real CLI must fail BEFORE recursive chmod/rm or any sctl call.
      writeFileSync(join(root, 'state.json'), '{"root":"placeholder"}');
      const result = spawnSync(process.execPath, [
        'scripts/e2e-native.mjs',
        'down',
        root,
      ], { encoding: 'utf8' });
      expect(result.status).not.toBe(0);
      expect(existsSync(join(root, 'state.json'))).toBe(true);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('refuses a symlink alias even when its destination has the expected name', () => {
    const root = mkdtempSync('/tmp/wacc-');
    const alias = mkdtempSync('/tmp/wacc-');
    rmSync(alias, { recursive: true });
    symlinkSync(root, alias);
    try {
      expect(() => assertManagedSandbox(alias, host)).toThrow();
    } finally {
      rmSync(alias);
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('refuses a sandbox directory that is group/world accessible', () => {
    const root = mkdtempSync('/tmp/wacc-');
    try {
      chmodSync(root, 0o755);
      expect(() => assertManagedSandbox(root, host)).toThrow(/unsafe/);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('refuses a correctly named sandbox without its native E2E state marker', () => {
    const root = mkdtempSync('/tmp/wacc-');
    try {
      writeFileSync(join(root, 'state.json'), JSON.stringify({
        root,
        workspace: join(root, 'workspace'),
        pids: {},
        groups: [],
      }));
      const result = spawnSync(process.execPath, [
        'scripts/e2e-native.mjs',
        'down',
        root,
      ], { encoding: 'utf8' });
      expect(result.status).not.toBe(0);
      expect(existsSync(join(root, 'state.json'))).toBe(true);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});

describe('sandboxEnv', () => {
  const root = '/tmp/claude-1000/wacc-abc123';
  const env = sandboxEnv({
    root,
    fixtureDir: '/tmp/claude-1000/wacc-abc123/fixture',
    scenario: 'happy',
    display: ':77',
  });

  it('points every home and XDG directory inside the sandbox', () => {
    for (const key of [
      'HOME',
      'XDG_RUNTIME_DIR',
      'XDG_CONFIG_HOME',
      'XDG_DATA_HOME',
      'XDG_STATE_HOME',
      'XDG_CACHE_HOME',
    ]) {
      expect(env[key].startsWith(root)).toBe(true);
    }
  });

  it('puts the sandbox bin first on PATH and nothing from the real session', () => {
    expect(env.PATH).toBe(`${root}/bin:/usr/bin:/bin`);
    expect(Object.keys(env)).not.toContain('DBUS_SESSION_BUS_ADDRESS');
  });

  it('points the Codex home at the sandbox, never at the real one', () => {
    expect(env.CODEX_HOME).toBe(`${root}/codex-home`);
    expect(env.WYSS_FIXTURE_USER_HOME).toBe(`${root}/codex-home`);
    expect(env.CODEX_HOME.startsWith(real.home)).toBe(false);
  });
});

describe('fixture markers', () => {
  it('finds every marker in a buffer that contains them', () => {
    const bytes = Buffer.from(`x ${FIXTURE_MARKERS.join(' y ')} z`, 'latin1');
    expect(fixtureHits(bytes)).toEqual(FIXTURE_MARKERS);
  });

  it('finds nothing in a buffer without them', () => {
    expect(fixtureHits(Buffer.from('ordinary release bytes'))).toEqual([]);
  });

  describe('checker control', () => {
    const dir = mkdtempSync(join(tmpdir(), 'wacc-guard-'));
    const withFixture = join(dir, 'wyss-console');
    const clean = join(dir, 'wyss-control');
    writeFileSync(withFixture, `binary ${FIXTURE_MARKERS[0]} tail`);
    writeFileSync(clean, 'binary without test code');
    writeFileSync(join(dir, 'notes.txt'), FIXTURE_MARKERS[0]);
    afterAll(() => rmSync(dir, { recursive: true, force: true }));

    it('flags a file that carries the fixture marker (positive control)', () => {
      expect(fixtureHitsInFile(withFixture)).toEqual([FIXTURE_MARKERS[0]]);
    });

    it('passes a file without the marker (negative control)', () => {
      expect(fixtureHitsInFile(clean)).toEqual([]);
    });

    it('finds product binaries only, not other files that mention a marker', () => {
      expect(productBinaries(dir).sort()).toEqual([clean, withFixture].sort());
    });
  });
});

describe('installFixtureCodex', () => {
  it('copies the fixture into the sandbox as an owner-only executable', () => {
    const root = mkdtempSync(join(tmpdir(), 'wacc-codex-'));
    const bin = join(root, 'bin');
    mkdirSync(bin);
    const installed = installFixtureCodex(root, fixturePath);
    expect(installed).toBe(join(bin, 'codex'));
    expect(readFileSync(installed, 'utf8')).toBe(
      readFileSync(fixturePath, 'utf8'),
    );
    expect(statSync(installed).mode & 0o777).toBe(0o700);
    rmSync(root, { recursive: true, force: true });
  });
});

describe('production package', () => {
  it.runIf(existsSync(packageDir))(
    'contains no fixture marker in any product binary',
    () => {
      const binaries = productBinaries(packageDir);
      expect(binaries.length).toBeGreaterThanOrEqual(2);
      for (const binary of binaries) {
        expect(fixtureHitsInFile(binary), binary).toEqual([]);
      }
    },
  );
});
