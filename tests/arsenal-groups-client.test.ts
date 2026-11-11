import { afterEach, expect, it } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { arsenalClient } from '../src/lib/desktop/arsenal-client';
import type { Item } from '../src/lib/contracts/arsenal';

afterEach(() => clearMocks());

type Call = { cmd: string; args: unknown };

/** A draft that holds nothing yet: the desktop answers the same shape it answers for a saved one. */
const emptyDraft = {
  selection: {
    id: '',
    name: '',
    provider: 'claude_code',
    items: [],
    composites: [],
    created_at: 0,
    updated_at: 0,
  },
  status: 'not_applied',
  items: [],
  conflicts: [],
  dependencies: 'analysed',
};

it('plans a draft preset through its own command and reads the preview', async () => {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return emptyDraft;
  });
  const items: Item[] = [{ capability: 'cap-1', revision: null }];
  const plan = await arsenalClient.planPreset('claude_code', items, ['gsd']);
  expect(calls).toEqual([
    {
      cmd: 'arsenal_preset_plan',
      args: {
        request: { provider: 'claude_code', items, composites: ['gsd'] },
      },
    },
  ]);
  expect(plan.status).toBe('not_applied');
});

it('reads the group corrections as one map from identity to group', async () => {
  mockIPC((cmd) => (cmd === 'arsenal_groups' ? { 'id-1': 'design' } : null));
  expect(await arsenalClient.groups()).toEqual({ 'id-1': 'design' });
});

it('refuses a groups answer that is not a map of group names', async () => {
  mockIPC(() => ({ 'id-1': 7 }));
  await expect(arsenalClient.groups()).rejects.toThrow();
});

it('moves a capability into a group and back to its automatic group', async () => {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return null;
  });
  await arsenalClient.moveGroup('id-1', 'design');
  await arsenalClient.moveGroup('id-1', null);
  expect(calls).toEqual([
    {
      cmd: 'arsenal_group_move',
      args: { request: { identity: 'id-1', group_id: 'design' } },
    },
    {
      cmd: 'arsenal_group_move',
      args: { request: { identity: 'id-1', group_id: null } },
    },
  ]);
});
