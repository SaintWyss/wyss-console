import { spawnSync } from 'node:child_process';
import { createServer } from 'node:net';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { assertOwnedRuntime } from '../scripts/e2e-guard.mjs';

const created: string[] = [];
const uid = process.getuid?.() ?? -1;

function scratch(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  created.push(dir);
  return dir;
}

afterAll(() => {
  for (const path of created) rmSync(path, { recursive: true, force: true });
});

/** A sandbox-shaped root whose run directory is private and owned by this user. */
function rootWithRun(mode = 0o700): string {
  const root = scratch('runtime-root-');
  const run = join(root, 'run');
  mkdirSync(run);
  chmodSync(run, mode);
  return root;
}

describe('assertOwnedRuntime: what a service command reaches through XDG_RUNTIME_DIR', () => {
  it('accepts a private runtime directory owned by this user', () => {
    expect(() => assertOwnedRuntime(rootWithRun(), { uid })).not.toThrow();
  });

  it('accepts a sandbox that has no bus yet', () => {
    expect(() => assertOwnedRuntime(rootWithRun(), { uid })).not.toThrow();
  });

  it('refuses a runtime directory that is a link, even to a private directory', () => {
    const root = scratch('runtime-link-root-');
    const target = scratch('runtime-link-target-');
    chmodSync(target, 0o700);
    symlinkSync(target, join(root, 'run'));
    expect(() => assertOwnedRuntime(root, { uid })).toThrow(
      /not a private directory owned by this user/,
    );
  });

  it('refuses a runtime directory that others can reach', () => {
    expect(() => assertOwnedRuntime(rootWithRun(0o755), { uid })).toThrow(
      /not a private directory owned by this user/,
    );
  });

  it('refuses a runtime directory owned by another user', () => {
    expect(() => assertOwnedRuntime(rootWithRun(), { uid: uid + 1 })).toThrow(
      /not a private directory owned by this user/,
    );
  });

  it('refuses a missing runtime directory: it fails closed', () => {
    expect(() =>
      assertOwnedRuntime(scratch('runtime-none-'), { uid }),
    ).toThrow();
  });

  it('refuses a bus that is a plain file, not a socket', () => {
    const root = rootWithRun();
    writeFileSync(join(root, 'run', 'bus'), '');
    expect(() => assertOwnedRuntime(root, { uid })).toThrow(/not a socket/);
  });

  it('accepts an owned real UNIX manager socket before service commands', async () => {
    const root = rootWithRun();
    const systemd = join(root, 'run', 'systemd');
    mkdirSync(systemd, { mode: 0o700 });
    const server = createServer();
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(join(systemd, 'private'), resolve);
    });
    try {
      expect(() =>
        assertOwnedRuntime(root, { uid }, { requireManager: true }),
      ).not.toThrow();
    } finally {
      await new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      });
    }
  });

  it('refuses a symlinked manager private socket before any service call', () => {
    const root = rootWithRun();
    const systemd = join(root, 'run', 'systemd');
    mkdirSync(systemd, { mode: 0o700 });
    const outside = scratch('runtime-private-target-');
    symlinkSync(join(outside, 'private'), join(systemd, 'private'));
    expect(() =>
      assertOwnedRuntime(root, { uid }, { requireManager: true }),
    ).toThrow(/private manager socket/);
  });

  it('refuses missing manager private socket when service access is requested', () => {
    const root = rootWithRun();
    const systemd = join(root, 'run', 'systemd');
    mkdirSync(systemd, { mode: 0o700 });
    expect(() =>
      assertOwnedRuntime(root, { uid }, { requireManager: true }),
    ).toThrow(/private manager socket/);
  });

  it('refuses a service-manager directory that is a link', () => {
    const root = rootWithRun();
    const target = scratch('runtime-systemd-target-');
    symlinkSync(target, join(root, 'run', 'systemd'));
    expect(() => assertOwnedRuntime(root, { uid })).toThrow(
      /not a plain directory/,
    );
  });
});

describe('down: a runtime that is a link never reaches a service manager', () => {
  it('refuses before calling systemctl, and leaves the link target alone', () => {
    // A sandbox named as the harness names one, so that only the runtime check can refuse it.
    const sandbox = mkdtempSync('/tmp/wacc-');
    created.push(sandbox);
    const target = scratch('runtime-cli-target-');
    writeFileSync(join(target, 'keep'), 'untouched');
    const marker = join(target, 'systemctl-called');
    symlinkSync(target, join(sandbox, 'run'));
    mkdirSync(join(sandbox, 'bin'));
    // A stand-in `systemctl` inside the sandbox's own PATH. If the guard fails, the stand-in
    // runs and leaves a marker; the real service manager is never reached either way.
    writeFileSync(
      join(sandbox, 'bin', 'systemctl'),
      `#!/bin/sh\ntouch '${marker}'\nexit 0\n`,
      { mode: 0o700 },
    );
    mkdirSync(join(sandbox, 'workspace'));
    writeFileSync(
      join(sandbox, 'state.json'),
      JSON.stringify({
        harness: 'wyss-console-native-e2e-v1',
        root: sandbox,
        pids: { systemd: 999999 },
        groups: [],
        workspace: join(sandbox, 'workspace'),
      }),
    );
    writeFileSync(
      join(sandbox, 'ledger.json'),
      JSON.stringify([{ name: 'systemd', pid: 999999, startTime: '1' }]),
    );

    const result = spawnSync(
      process.execPath,
      [resolve('scripts/e2e-native.mjs'), 'down', sandbox],
      { encoding: 'utf8' },
    );

    expect(result.status).not.toBe(0);
    expect(result.stderr).toMatch(/not a private directory owned by this user/);
    expect(existsSync(marker)).toBe(false);
    expect(readFileSync(join(target, 'keep'), 'utf8')).toBe('untouched');
    expect(existsSync(sandbox)).toBe(true);
  });
});
