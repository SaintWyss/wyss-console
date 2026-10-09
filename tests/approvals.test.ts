import { afterEach, expect, it } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  APPROVAL_REASONS,
  APPROVAL_STATES,
  parseApprovalDetail,
  parseApprovalList,
  parseApprovalSummary,
  parseDecisionReceipt,
} from '../src/lib/contracts/approvals';
import { parseManagedApprovalId } from '../src/lib/contracts/identities';
import {
  approveApproval,
  denyApproval,
  readApproval,
  readApprovals,
} from '../src/lib/desktop/approvals-client';
import { SessionCallError } from '../src/lib/desktop/sessions-client';
import fixture from './fixtures/approval-list-v1.json';

const COMMAND = parseManagedApprovalId(fixture.command.approval.id);
const FILE = parseManagedApprovalId(fixture.file.approval.id);
const OTHER = '99999999-9999-4999-8999-999999999999';
const list = {
  schema_version: 1,
  head: '14',
  pending: [fixture.command],
  recent: fixture.recent,
  partial: false,
};
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
/** `base` with one path replaced, or removed when `value` is undefined. */
function tampered(base: unknown, path: (string | number)[], value: unknown) {
  const copy = clone(base) as Record<string | number, unknown>;
  let node = copy;
  for (const key of path.slice(0, -1))
    node = node[key] as Record<string | number, unknown>;
  const last = path.at(-1)!;
  if (value === undefined) delete node[last];
  else node[last] = value;
  return copy;
}

afterEach(() => clearMocks());

it('accepts the projection the desktop sends and nothing beside it', () => {
  expect(parseApprovalList(fixture.empty)).toEqual(fixture.empty);
  expect(parseApprovalList(list)).toEqual(list);
  expect(parseApprovalDetail(fixture.command)).toEqual(fixture.command);
  expect(parseApprovalDetail(fixture.file)).toEqual(fixture.file);
  for (const ended of fixture.recent)
    expect(parseApprovalSummary(ended)).toEqual(ended);
  // The authority's own names. Only `pending` is not final.
  expect(APPROVAL_STATES).toEqual([
    'pending',
    'accepted',
    'declined',
    'cancelled',
    'expired',
    'invalidated',
    'indeterminate',
  ]);
  expect(APPROVAL_REASONS).toHaveLength(13);
  expect(parseDecisionReceipt({ id: COMMAND, state: 'accepted' })).toEqual({
    id: COMMAND,
    state: 'accepted',
  });
});

it('refuses an approval that carries anything a decision has no use for', () => {
  // The provider's item, the lease, the runtime, the fingerprint, the effect marker,
  // the counts, the parsed labels, a raw frame: none has a place in the projection.
  for (const extra of [
    'provider_item_id',
    'admitted_lease_id',
    'admitted_lease_revision',
    'lease_id',
    'runtime_instance_id',
    'action_fingerprint',
    'fingerprint',
    'effect',
    'structure',
    'parsed_actions',
    'environment',
    'raw',
    'reasoning',
  ]) {
    expect(() =>
      parseApprovalDetail({ ...fixture.command, [extra]: 'x' }),
    ).toThrow();
    for (const part of ['approval', 'command'])
      expect(() =>
        parseApprovalDetail(tampered(fixture.command, [part, extra], 'x')),
      ).toThrow();
    expect(() =>
      parseApprovalDetail(tampered(fixture.file, ['file', extra], 'x')),
    ).toThrow();
    expect(() => parseApprovalList({ ...list, [extra]: 'x' })).toThrow();
    expect(() =>
      parseApprovalList(tampered(list, ['recent', 0, extra], 'x')),
    ).toThrow();
    expect(() =>
      parseDecisionReceipt({ id: COMMAND, state: 'accepted', [extra]: 'x' }),
    ).toThrow();
  }
  // A state, a reason, a kind or a decision the contract has no name for is not
  // guessed at: least of all one that would widen what a decision means.
  const summary = (key: string, value: unknown) => () =>
    parseApprovalSummary({ ...fixture.recent[0], [key]: value });
  expect(summary('state', 'approved')).toThrow(/invalid_state/);
  expect(summary('reason', 'because')).toThrow(/invalid_reason/);
  expect(summary('kind', 'network_access')).toThrow(/invalid_kind/);
  for (const decision of [
    'accept_for_session',
    'acceptForSession',
    'always',
    'acceptWithExecpolicyAmendment',
  ])
    expect(summary('decision', decision)).toThrow(/invalid_decision/);
  expect(() =>
    parseApprovalDetail(tampered(fixture.file, ['file', 'kind'], 'chmod')),
  ).toThrow(/invalid_file_kind/);
});

it('refuses an approval whose parts contradict each other', () => {
  const command = (path: (string | number)[], value: unknown) => () =>
    parseApprovalDetail(tampered(fixture.command, path, value));
  const file = (path: (string | number)[], value: unknown) => () =>
    parseApprovalDetail(tampered(fixture.file, path, value));
  // Pending says neither when nor why it ended; ended says both.
  expect(command(['approval', 'ended_at'], '2026-10-09T10:11:00Z')).toThrow(
    /invalid_end/,
  );
  expect(command(['approval', 'reason'], 'client_decision')).toThrow(
    /invalid_end/,
  );
  expect(() =>
    parseApprovalSummary({ ...fixture.recent[0], ended_at: null }),
  ).toThrow(/invalid_end/);
  // What was decided is what the state says was decided, and nothing is decided
  // while it is pending.
  expect(command(['approval', 'decision'], 'accept')).toThrow(
    /invalid_decision/,
  );
  for (const [state, decision] of [
    ['accepted', 'decline'],
    ['accepted', null],
    ['declined', 'accept'],
    ['expired', 'accept'],
    ['invalidated', 'cancel'],
    ['indeterminate', null],
  ])
    expect(() =>
      parseApprovalSummary({ ...fixture.recent[0], state, decision }),
    ).toThrow(/invalid_decision/);
  // No hint offers a decision the shown state already rules out.
  expect(() =>
    parseApprovalSummary({ ...fixture.recent[0], decidable: true }),
  ).toThrow(/invalid_hints/);
  expect(command(['approval', 'revision'], 0)).toThrow(/invalid_revision/);
  expect(command(['approval', 'revision'], '1')).toThrow(/invalid_revision/);
  expect(command(['approval', 'id'], 'approval-1')).toThrow(/invalid_identity/);
  expect(command(['approval', 'turn'], null)).toThrow(/invalid_identity/);
  expect(command(['approval', 'expires_at'], 'soon')).toThrow(/expires_at/);

  // The action is of the approval's own kind.
  expect(command(['approval', 'kind'], 'file_change')).toThrow(
    /invalid_action/,
  );
  expect(file(['approval', 'kind'], 'command_execution')).toThrow(
    /invalid_action/,
  );
  expect(command(['files'], 1)).toThrow(/invalid_action/);
  expect(file(['files'], 0)).toThrow(/invalid_action/);
  expect(file(['files'], null)).toThrow(/invalid_action/);
  // A file is one of its change set.
  expect(file(['file', 'index'], 2)).toThrow(/invalid_action/);
  expect(file(['files'], 65)).toThrow(/invalid_files/);
  // One that can be decided has its action; one that ended has none to show.
  expect(command(['command'], null)).toThrow(/invalid_action/);
  expect(() =>
    parseApprovalDetail({
      ...fixture.command,
      approval: fixture.recent[0],
    }),
  ).toThrow(/invalid_action/);
  expect(() =>
    parseApprovalDetail({
      approval: fixture.recent[0],
      workspace: null,
      command: null,
      files: null,
      file: null,
    }),
  ).not.toThrow();

  // A command is whole and bounded, and its directory is an absolute path.
  expect(command(['command', 'command'], '')).toThrow(/invalid_command/);
  expect(command(['command', 'command'], 'x'.repeat(16 * 1024 + 1))).toThrow(
    /invalid_command/,
  );
  expect(command(['command', 'command'], 'a\u0000b')).toThrow(
    /invalid_command/,
  );
  expect(command(['command', 'command'], ['git', 'status'])).toThrow(
    /invalid_command/,
  );
  expect(command(['command', 'cwd'], 'relative')).toThrow(/invalid_cwd/);
  expect(command(['command', 'cwd'], '/a\u001b[31m')).toThrow(/invalid_cwd/);
  expect(command(['workspace'], 'relative')).toThrow(/invalid_workspace/);

  // A page of a diff is bounded, lies inside its diff, and the next one starts where
  // it ends: nothing is skipped and nothing is unbounded.
  expect(file(['file', 'chunk'], 'x'.repeat(8 * 1024 + 1))).toThrow(
    /invalid_chunk/,
  );
  expect(file(['file', 'bytes'], 128 * 1024 + 1)).toThrow(/invalid_bytes/);
  expect(file(['file', 'bytes'], 30)).toThrow(/invalid_page/);
  expect(file(['file', 'offset'], 5)).toThrow(/invalid_page/);
  expect(file(['file', 'next'], 22)).toThrow(/invalid_page/);
  expect(file(['file', 'next'], 10)).toThrow(/invalid_page/);
  expect(file(['file', 'offset'], -1)).toThrow(/invalid_offset/);
  expect(() =>
    parseApprovalDetail(
      tampered(fixture.file, ['file'], {
        ...fixture.file.file,
        bytes: 40,
        next: 22,
      }),
    ),
  ).not.toThrow();
  // A path is one line of text that names something.
  for (const path of ['', 'a\nb', '/x\u001b[2J', null, 7])
    expect(file(['file', 'path'], path)).toThrow(/invalid_path/);
  expect(file(['file', 'path'], `/${'x'.repeat(1024)}`)).toThrow(
    /invalid_path/,
  );
  // Only a move has a destination, and it always has one.
  expect(file(['file', 'move_path'], '/home/dev/elsewhere')).toThrow(
    /invalid_move_path/,
  );
  expect(file(['file', 'kind'], 'move')).toThrow(/invalid_move_path/);

  // A list holds what it says it holds, each approval once, and no more than a page.
  expect(() => parseApprovalList({ ...list, schema_version: 2 })).toThrow(
    /schema_version/,
  );
  expect(() =>
    parseApprovalList({ ...list, recent: [fixture.command.approval] }),
  ).toThrow(/invalid_list/);
  expect(() =>
    parseApprovalList({
      ...list,
      pending: [fixture.command, fixture.command],
    }),
  ).toThrow(/invalid_list/);
  expect(() =>
    parseApprovalList({
      ...list,
      recent: Array.from({ length: 8 }, () => fixture.recent[0]),
    }),
  ).toThrow(/invalid_list/);
  expect(() => parseApprovalList({ ...list, head: 14 })).toThrow(
    /invalid_head/,
  );
  expect(() => parseApprovalList({ ...list, partial: 'no' })).toThrow(
    /invalid_partial/,
  );
  // A receipt is of a decision the authority committed.
  expect(() =>
    parseDecisionReceipt({ id: COMMAND, state: 'pending' }),
  ).toThrow();
  expect(() => parseDecisionReceipt({ id: 'x', state: 'accepted' })).toThrow();
});

interface Call {
  command: string;
  payload: unknown;
}
function bridge(answer: (call: Call) => unknown) {
  const calls: Call[] = [];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    return answer({ command, payload });
  });
  return calls;
}
async function failure(call: Promise<unknown>) {
  try {
    await call;
  } catch (error) {
    expect(error).toBeInstanceOf(SessionCallError);
    return (error as SessionCallError).failure;
  }
  throw new Error('the call was answered');
}

it('sends two reads and two decisions, each as one narrow request', async () => {
  const calls = bridge(({ command, payload }) => {
    if (command === 'managed_approvals') return list;
    if (command !== 'managed_approval_read')
      return { id: COMMAND, state: 'accepted' };
    const id = (payload as { request: { approval_id: string } }).request
      .approval_id;
    return id === COMMAND ? fixture.command : fixture.file;
  });
  await readApprovals();
  await readApproval(COMMAND, null, null);
  await readApproval(FILE, 0, 0);
  await approveApproval('intent-1', COMMAND, 1);
  await denyApproval('intent-2', COMMAND, 1);
  expect(calls).toEqual([
    { command: 'managed_approvals', payload: { request: {} } },
    {
      command: 'managed_approval_read',
      payload: { request: { approval_id: COMMAND, file: null, offset: null } },
    },
    {
      command: 'managed_approval_read',
      payload: { request: { approval_id: FILE, file: 0, offset: 0 } },
    },
    {
      command: 'managed_approval_approve',
      payload: {
        request: {
          intent_id: 'intent-1',
          approval_id: COMMAND,
          expected_revision: 1,
        },
      },
    },
    {
      command: 'managed_approval_deny',
      payload: {
        request: {
          intent_id: 'intent-2',
          approval_id: COMMAND,
          expected_revision: 1,
        },
      },
    },
  ]);
  // A decision is the command it is sent to. No request carries one as data, nor a
  // command, a scope, a lease, a rule or anything that outlives the approval.
  expect(JSON.stringify(calls.map((c) => c.payload))).not.toMatch(
    /decision|accept|decline|command|scope|lease|always|remember|rule|policy|method|fingerprint/,
  );
});

it('takes an answer about another approval, file or page for a failed bridge', async () => {
  const internal = { code: 'internal', retry: false };
  bridge(() => fixture.command);
  // Another approval than the one that was asked for.
  expect(await failure(readApproval(FILE, null, null))).toEqual(internal);
  bridge(() => fixture.file);
  // Another file, or another page, than the one that was asked for.
  expect(await failure(readApproval(FILE, 1, 0))).toEqual(internal);
  expect(await failure(readApproval(FILE, 0, 8192))).toEqual(internal);
  expect((await readApproval(FILE, 0, 0)).file?.index).toBe(0);
  // A receipt for another approval.
  bridge(() => ({ id: OTHER, state: 'accepted' }));
  expect(await failure(approveApproval('i', COMMAND, 1))).toEqual(internal);
  expect(await failure(denyApproval('i', COMMAND, 1))).toEqual(internal);
  // Not the projection at all: the canonical record does not pass for it.
  bridge(() => ({
    approval: {
      managed_approval_id: COMMAND,
      state: 'pending',
      action_fingerprint: 'ab'.repeat(32),
    },
    acceptable: true,
    command: { command: 'git status', cwd: '/x', parsed_actions: [] },
  }));
  expect(await failure(readApproval(COMMAND, null, null))).toEqual(internal);
  expect(await failure(readApprovals())).toEqual(internal);
});

it('reports a refusal by the name the desktop gave it, and anything else as internal', async () => {
  bridge(({ command }) => {
    throw command === 'managed_approval_approve'
      ? { code: 'changed', retry: false }
      : command === 'managed_approval_deny'
        ? { code: 'not_allowed', retry: false }
        : command === 'managed_approvals'
          ? { code: 'unavailable', retry: true }
          : 'managedTurn/approval/status failed: provider item call-1';
  });
  expect(await failure(approveApproval('i', COMMAND, 1))).toEqual({
    code: 'changed',
    retry: false,
  });
  expect(await failure(denyApproval('i', COMMAND, 1))).toEqual({
    code: 'not_allowed',
    retry: false,
  });
  expect(await failure(readApprovals())).toEqual({
    code: 'unavailable',
    retry: true,
  });
  expect(await failure(readApproval(COMMAND, null, null))).toEqual({
    code: 'internal',
    retry: false,
  });
});
