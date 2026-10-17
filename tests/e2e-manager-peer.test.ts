import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { identityOf } from '../scripts/e2e-process-ledger.mjs';
import { verifyManagerPeer } from '../scripts/e2e-manager-peer.mjs';

const roots: string[] = [];
const children: number[] = [];

function rootWithSocketDir(): string {
  const root = mkdtempSync(join(tmpdir(), 'e2e-peer-'));
  roots.push(root);
  mkdirSync(join(root, 'run', 'systemd'), { recursive: true });
  return root;
}

async function serverAt(root: string) {
  const server = createServer((socket) => socket.end());
  await new Promise<void>((done, reject) => {
    server.once('error', reject);
    server.listen(join(root, 'run', 'systemd', 'private'), done);
  });
  return server;
}

function ledger(root: string, pid: number) {
  const startTime = identityOf(pid);
  if (startTime === null) throw new Error('test process has exited');
  writeFileSync(
    join(root, 'ledger.json'),
    JSON.stringify([{ name: 'systemd', pid, startTime }]),
  );
}

afterAll(() => {
  for (const pid of children) {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {
      // Already gone.
    }
  }
  for (const root of roots) rmSync(root, { recursive: true, force: true });
});

describe('private systemd peer identity', () => {
  it('accepts a socket served by the exact live process in the ledger', async () => {
    const root = rootWithSocketDir();
    const server = await serverAt(root);
    try {
      ledger(root, process.pid);
      expect(verifyManagerPeer(root)).toEqual({
        pid: process.pid,
        uid: process.getuid?.(),
      });
    } finally {
      await new Promise<void>((done) => server.close(() => done()));
    }
  });

  it('rejects a same-UID relay whose PID differs from the recorded manager', async () => {
    const root = rootWithSocketDir();
    const server = await serverAt(root);
    const child = spawn('sleep', ['120'], { stdio: 'ignore' });
    if (child.pid === undefined) throw new Error('child did not spawn');
    children.push(child.pid);
    try {
      ledger(root, child.pid);
      expect(() => verifyManagerPeer(root)).toThrow(
        /served by another process/,
      );
    } finally {
      await new Promise<void>((done) => server.close(() => done()));
    }
  });

  it('rejects a missing or ambiguous recorded manager', () => {
    const root = rootWithSocketDir();
    expect(() => verifyManagerPeer(root)).toThrow(/identity is absent/);
    const startTime = identityOf(process.pid);
    writeFileSync(
      join(root, 'ledger.json'),
      JSON.stringify([
        { name: 'systemd', pid: process.pid, startTime },
        { name: 'systemd', pid: process.pid, startTime },
      ]),
    );
    expect(() => verifyManagerPeer(root)).toThrow(/identity is absent/);
  });
});
