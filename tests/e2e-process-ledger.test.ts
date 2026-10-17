import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
  identityOf,
  ledgerPath,
  readLedger,
  recordProcess,
  terminateRecorded,
  withCleanup,
} from '../scripts/e2e-process-ledger.mjs';

const roots: string[] = [];
const groups: number[] = [];

function sandbox(): string {
  const root = mkdtempSync(join(tmpdir(), 'ledger-test-'));
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

describe('recording', () => {
  it('writes each process to the ledger at spawn, with its start time', () => {
    const root = sandbox();
    const pid = spawnGroup('sleep', ['120']);
    recordProcess(root, 'sleeper', pid);

    const [entry] = readLedger(root);
    expect(entry).toMatchObject({ name: 'sleeper', pid });
    expect(entry.startTime).toBe(identityOf(pid));
    expect(readFileSync(ledgerPath(root), 'utf8')).toContain('"sleeper"');
  });

  it('refuses to record a process that has already exited', async () => {
    const root = sandbox();
    const child = spawn('true', [], { stdio: 'ignore' });
    await new Promise((done) => child.on('exit', done));
    expect(() => recordProcess(root, 'gone', child.pid as number)).toThrow(
      /exited before it could be recorded/,
    );
  });
});

describe('ledger integrity', () => {
  it('refuses malformed JSON instead of treating active processes as absent', async () => {
    const root = sandbox();
    writeFileSync(ledgerPath(root), '{incomplete');
    expect(() => readLedger(root)).toThrow(/invalid process ledger/);
    await expect(terminateRecorded(root)).rejects.toThrow(
      /invalid process ledger/,
    );
  });

  it('refuses malformed entries rather than signaling unverified groups', async () => {
    const root = sandbox();
    writeFileSync(
      ledgerPath(root),
      JSON.stringify([{ name: 'untrusted', pid: -1, startTime: 'not-a-time' }]),
    );
    expect(() => readLedger(root)).toThrow(/invalid process ledger/);
    await expect(terminateRecorded(root)).rejects.toThrow(
      /invalid process ledger/,
    );
  });
});

describe('terminating', () => {
  it('stops a recorded group, including members that outlive their leader', async () => {
    const root = sandbox();
    // The shell is the leader; the two sleeps are its members and it exits after them.
    const pid = spawnGroup('sh', ['-c', 'sleep 120 & sleep 120; wait']);
    recordProcess(root, 'shell-group', pid);
    await settle(200);

    const report = await terminateRecorded(root, { graceMs: 2000 });

    expect(report).toEqual([{ name: 'shell-group', pid, outcome: 'stopped' }]);
    expect(groupAlive(pid)).toBe(false);
  });

  it('never signals a PID whose start time no longer matches the record', async () => {
    const root = sandbox();
    const pid = spawnGroup('sleep', ['120']);
    recordProcess(root, 'reused-pid', pid);
    // Simulates a recorded PID that the kernel gave to another process.
    const entries = readLedger(root);
    entries[0].startTime = '0';
    writeFileSync(ledgerPath(root), JSON.stringify(entries));

    const report = await terminateRecorded(root, { graceMs: 500 });

    expect(report).toEqual([
      { name: 'reused-pid', pid, outcome: 'identity-changed' },
    ]);
    expect(groupAlive(pid)).toBe(true);
    process.kill(-pid, 'SIGKILL');
  });

  it('reports already-gone for a recorded process that has exited', async () => {
    const root = sandbox();
    const pid = spawnGroup('sleep', ['0.1']);
    recordProcess(root, 'short', pid);
    await settle(600);

    const report = await terminateRecorded(root);

    expect(report).toEqual([{ name: 'short', pid, outcome: 'already-gone' }]);
  });
});

describe('failed startup', () => {
  it('stops every process recorded before the failure and rethrows it', async () => {
    const root = sandbox();
    let started = 0;

    await expect(
      withCleanup(root, async () => {
        started = recordProcess(root, 'first', spawnGroup('sleep', ['120']));
        recordProcess(root, 'second', spawnGroup('sleep', ['120']));
        throw new Error('the compositor did not start');
      }),
    ).rejects.toThrow('the compositor did not start');

    expect(readLedger(root)).toHaveLength(2);
    for (const entry of readLedger(root)) {
      expect(groupAlive(entry.pid)).toBe(false);
    }
    expect(groupAlive(started)).toBe(false);
  });

  it('leaves the ledger untouched when the startup succeeds', async () => {
    const root = sandbox();
    const pid = spawnGroup('sleep', ['120']);

    const value = await withCleanup(root, async () => {
      recordProcess(root, 'kept', pid);
      return 'up';
    });

    expect(value).toBe('up');
    expect(groupAlive(pid)).toBe(true);
    process.kill(-pid, 'SIGKILL');
  });
});
