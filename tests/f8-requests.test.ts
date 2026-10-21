import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, expect, it } from 'vitest';
import type { Fence } from '../src/lib/contracts/f8';
import type {
  ManagedApprovalId,
  ManagedSessionId,
  ManagedTurnId,
} from '../src/lib/contracts/identities';
import * as client from '../src/lib/desktop/f8-client';

// What the window sends, command by command, is written to a fixture that the Rust tests
// read with the very types the commands take. A key renamed on one side and not the other
// fails there, not in a window.
const FIXTURE = join(__dirname, 'fixtures', 'f8-requests-v1.json');
const UUID = (n: number) =>
  `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`;
const session = UUID(1) as ManagedSessionId;
const turn = UUID(2) as ManagedTurnId;
const approval = UUID(3) as ManagedApprovalId;
const lease = UUID(4);
const intent = UUID(5);
const fence: Fence = {
  session,
  generation: 1,
  instance: UUID(6) as Fence['instance'],
};

afterEach(() => mockIPC(() => undefined));

it('sends exactly the requests the commands accept', async () => {
  const sent: Record<string, unknown> = {};
  mockIPC((command, payload) => {
    sent[command] =
      (payload as { request?: unknown } | undefined)?.request ?? null;
    return {};
  });
  const calls: (() => Promise<unknown>)[] = [
    () => client.readF8State(),
    () => client.reconnectF8(),
    () => client.readSessions('7'),
    () => client.readSession(session),
    () => client.prepareSession(intent),
    () => client.closeSession(intent, fence, 1),
    () =>
      client.startRuntime(
        intent,
        session,
        0,
        0,
        'local_tools',
        '/home/dev/project',
      ),
    () => client.stopRuntime(intent, fence, 1),
    () => client.readModels(fence),
    () =>
      client.grantLease(intent, fence, 1, ['turn_start', 'file_change'], 300),
    () => client.renewLease(intent, fence, lease, 1, 600),
    () => client.revokeLease(intent, lease, 1),
    () => client.createBinding(intent, fence, 1, '/home/dev/project'),
    () => client.resumeBinding(intent, fence, 1),
    () => client.abandonBinding(intent, fence, UUID(7)),
    () => client.startTurn(intent, fence, 1, 'work'),
    () => client.cancelTurn(intent, fence, turn, 2),
    () => client.readTurn(turn, '3', true),
    () => client.readApproval(approval, 1, 8192),
    () => client.decideApproval('accept', intent, fence, approval, 1),
    () => client.decideApproval('decline', intent, fence, approval, 1),
    () => client.decideApproval('cancel', intent, fence, approval, 1),
    () => client.readAudit({ stream_id: UUID(8), before: '9' }),
    () => client.waitAudit(UUID(8), '9', 20000),
  ];
  // The answers are not the projections, so every call rejects: only what it sent matters.
  for (const call of calls) await call().catch(() => undefined);
  if (process.env.WYSS_F8_WRITE_REQUESTS)
    writeFileSync(FIXTURE, JSON.stringify(sent, null, 2) + '\n');
  expect(sent).toEqual(JSON.parse(readFileSync(FIXTURE, 'utf8')));
  // Every command of the interface is asked for.
  expect(Object.keys(sent)).toHaveLength(24);
});
