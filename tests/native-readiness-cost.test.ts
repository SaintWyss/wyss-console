import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  NativeController,
  type NativeState,
} from '../src/lib/features/native/controller';
import { readLauncher, readReadiness } from '../src/lib/desktop/native-client';
import { parseReadiness, type Readiness } from '../src/lib/contracts/native';
import { fakeClient, launcherWithM9 } from './native-harness';

let root: string;
let controller: NativeController;
let client: ReturnType<typeof fakeClient>;
let status: Readiness['m9_native_status'];
let fail: boolean;
let pending: Promise<Readiness> | null;
let requests: string[];
let publish: ReturnType<typeof vi.fn<(state: NativeState) => void>>;
const settle = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
};
const probes = () =>
  readFileSync(join(root, 'probes'), 'utf8').trim().split('\n');
beforeEach(() => {
  vi.useFakeTimers();
  root = mkdtempSync(join(tmpdir(), 'wyss-readiness-cost-'));
  status = 'ready';
  fail = false;
  pending = null;
  requests = [];
  for (const provider of ['claude', 'codex', 'opencode']) {
    writeFileSync(
      join(root, provider),
      `#!/bin/sh\nprintf '%s\\n' "$1" >> '${join(root, 'probes')}'\n[ "$1" = '--version' ] || exit 81\necho fixture-version\n`,
      { mode: 0o700 },
    );
  }
  mockIPC((command) => {
    requests.push(command);
    if (command === 'native_launcher') {
      for (const provider of ['claude', 'codex', 'opencode'])
        execFileSync(join(root, provider), ['--version']);
      return launcherWithM9('ready');
    }
    if (command === 'native_readiness') {
      if (fail) throw { code: 'host_outdated', reasons: [] };
      return pending ?? { m9_native_status: status };
    }
    throw new Error(`unexpected IPC: ${command}`);
  });
  publish = vi.fn();
  client = fakeClient({ launcher: readLauncher, readiness: readReadiness });
  controller = new NativeController(publish, undefined, client);
});
afterEach(() => {
  controller.dispose();
  clearMocks();
  vi.useRealTimers();
  rmSync(root, { recursive: true, force: true });
});

it('A/F: 60 simulated seconds cost exactly 30 lightweight reads and zero heartbeat processes/version probes', async () => {
  controller.setVisible(true);
  await settle();
  expect(probes()).toEqual(['--version', '--version', '--version']);
  requests = [];
  await vi.advanceTimersByTimeAsync(60_000);
  expect(requests).toEqual(Array<string>(30).fill('native_readiness'));
  expect(probes()).toHaveLength(3);
  expect(client.sessions).toHaveBeenCalledTimes(31);
  expect(controller.state().m9Blocked).toBe(false);
});

it('B/C: the next heartbeat detects degradation and real backend recovery without discovery', async () => {
  controller.setVisible(true);
  await settle();
  requests = [];
  status = 'storage_unavailable';
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().launcher?.m9_native_status).toBe(status);
  for (const agent of ['opencode', 'hermes'] as const)
    expect(await controller.startAgent(agent, '/home/u', null)).toBe(false);
  status = 'ready';
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().m9Blocked).toBe(false);
  expect(requests).toEqual(['native_readiness', 'native_readiness']);
  expect(probes()).toHaveLength(3);
});

it('D: unsupported/failed checks withdraw confidence without claiming storage degradation or blocking legacy', async () => {
  controller.setVisible(true);
  await settle();
  fail = true;
  await vi.advanceTimersByTimeAsync(2000);
  expect(controller.state().m9Uncertain).toBe(true);
  expect(controller.state().m9Blocked).toBe(true);
  expect(controller.state().launcher?.m9_native_status).toBe('ready');
  expect(await controller.startAgent('opencode', '/home/u', null)).toBe(false);
  expect(await controller.startAgent('claude_code', '/home/u', null)).toBe(
    true,
  );
  expect(await controller.startAgent('codex', '/home/u', null)).toBe(true);
  expect(probes()).toHaveLength(3);
});

it('D/E: stalled readiness stays bounded, hide stops polling, show confirms, and dispose rejects late responses', async () => {
  controller.setVisible(true);
  await settle();
  let resolve!: (value: Readiness) => void;
  pending = new Promise((yes) => {
    resolve = yes;
  });
  requests = [];
  await vi.advanceTimersByTimeAsync(60_000);
  expect(requests).toEqual(['native_readiness']);
  expect(controller.state().m9Uncertain).toBe(true);
  controller.setVisible(false);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(requests).toHaveLength(1);
  controller.setVisible(true);
  expect(requests).toHaveLength(2);
  controller.dispose();
  const count = publish.mock.calls.length;
  pending = null;
  resolve({ m9_native_status: 'ready' });
  await settle();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(vi.getTimerCount()).toBe(0);
  expect(publish).toHaveBeenCalledTimes(count);
  expect(requests).toHaveLength(2);
  expect(probes()).toHaveLength(3);
});

it('rejects missing or malformed lightweight status rather than assuming ready', () => {
  for (const value of [null, {}, { m9_native_status: 'unknown' }])
    expect(() => parseReadiness(value)).toThrow();
});

it('attaches discovery before initial readiness and never lets catalogue status restore degraded storage', async () => {
  let answer!: (value: ReturnType<typeof launcherWithM9>) => void;
  const client = fakeClient({
    launcher: vi.fn(
      () =>
        new Promise<ReturnType<typeof launcherWithM9>>((yes) => {
          answer = yes;
        }),
    ),
    readiness: vi.fn(async () => ({
      m9_native_status: 'storage_unavailable' as const,
    })),
  });
  controller.dispose();
  controller = new NativeController(publish, undefined, client);
  controller.setVisible(true);
  await settle();
  expect(client.readiness).not.toHaveBeenCalled();
  expect(client.sessions).not.toHaveBeenCalled();
  await controller.refreshLauncher();
  expect(controller.state().m9Blocked).toBe(true);
  answer(launcherWithM9('ready'));
  await settle();
  expect(controller.state().launcher?.m9_native_status).toBe(
    'storage_unavailable',
  );
  expect(controller.state().m9Blocked).toBe(true);
});
