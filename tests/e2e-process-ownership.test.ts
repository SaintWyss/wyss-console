import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
  recordProcess,
  terminateRecorded,
} from '../scripts/e2e-process-ledger.mjs';

const roots: string[] = [];
const groups: number[] = [];

function sandbox(): string {
  const root = mkdtempSync(join(tmpdir(), 'ownership-test-'));
  roots.push(root);
  return root;
}

// A detached process group, as the harness starts its processes.
function spawnGroup(command: string, args: string[]): number {
  const child = spawn(command, args, { detached: true, stdio: 'ignore' });
  child.unref();
  if (child.pid === undefined) throw new Error(`could not start ${command}`);
  groups.push(child.pid);
  return child.pid;
}

function groupAlive(pid: number): boolean {
  try {
    process.kill(-pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function settle(ms: number): Promise<void> {
  await new Promise((done) => setTimeout(done, ms));
}

afterAll(() => {
  for (const pid of groups) {
    try {
      process.kill(-pid, 'SIGKILL');
    } catch {
      // Already gone.
    }
  }
  for (const root of roots) rmSync(root, { recursive: true, force: true });
});

describe('a group whose recorded leader is gone is not proof of ownership', () => {
  it('reports the remaining members and signals nothing', async () => {
    const root = sandbox();
    // The shell leaves a background sleep in its group and exits, so the leader is gone
    // while its member is still running.
    const pid = spawnGroup('sh', ['-c', 'sleep 120 & sleep 0.5']);
    recordProcess(root, 'orphaned-group', pid);
    await settle(1500);
    expect(groupAlive(pid)).toBe(true);

    const report = await terminateRecorded(root, { graceMs: 500 });

    expect(report).toEqual([
      { name: 'orphaned-group', pid, outcome: 'unverified-members' },
    ]);
    // The member was not signalled: it is still running and still ours to report.
    expect(groupAlive(pid)).toBe(true);
    process.kill(-pid, 'SIGKILL');
  });
});
