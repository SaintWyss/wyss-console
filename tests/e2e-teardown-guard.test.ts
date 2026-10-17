import { spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:net';
import { join, resolve } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { identityOf } from '../scripts/e2e-process-ledger.mjs';

const roots: string[] = [];
afterAll(() => {
  for (const root of roots) rmSync(root, { recursive: true, force: true });
});

describe('native teardown requires authority stop confirmation', () => {
  it('preserves the sandbox when the isolated authority stop command fails', async () => {
    const root = mkdtempSync('/tmp/wacc-');
    roots.push(root);
    for (const dir of ['bin', 'workspace', 'run', 'run/systemd']) {
      mkdirSync(join(root, dir), { recursive: true, mode: 0o700 });
    }
    // A synthetic user-manager socket served by this test process.
    const manager = createServer((socket) => socket.end());
    await new Promise<void>((done, reject) => {
      manager.once('error', reject);
      manager.listen(join(root, 'run', 'systemd', 'private'), done);
    });
    try {
      const startTime = identityOf(process.pid);
      if (startTime === null) throw new Error('test process has exited');
      writeFileSync(
        join(root, 'ledger.json'),
        JSON.stringify([{ name: 'systemd', pid: process.pid, startTime }]),
      );
      writeFileSync(
        join(root, 'state.json'),
        JSON.stringify({
          harness: 'wyss-console-native-e2e-v1',
          root,
          pids: { systemd: process.pid },
          groups: [],
          workspace: join(root, 'workspace'),
        }),
      );
      const fake = join(root, 'bin', 'systemctl');
      writeFileSync(
        fake,
        '#!/bin/sh\nif [ "$2" = "show" ]; then echo 0; exit 0; fi\nexit 7\n',
        { mode: 0o700 },
      );
      const run = spawnSync(
        process.execPath,
        [resolve('scripts/e2e-native.mjs'), 'down', root],
        { encoding: 'utf8', timeout: 10000 },
      );
      expect(run.status).not.toBe(0);
      expect(run.stderr).toContain('isolated authority stop failed');
      // Failed teardown must preserve its sandbox for inspection.
      expect(existsSync(join(root, 'state.json'))).toBe(true);
    } finally {
      await new Promise<void>((done) => manager.close(() => done()));
    }
  });
});
