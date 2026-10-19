/* eslint-disable @typescript-eslint/no-explicit-any -- damage is done to arbitrary JSON */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  parseApplied,
  parseConfigurationFailure,
  parseHistory,
  parsePlan,
  parseRecords,
} from '../src/lib/contracts/configuration';
import {
  ChangesController,
  type ChangesState,
} from '../src/lib/features/configuration/changes-controller';
import {
  CHANGE_FAILURE_TEXT,
  OUTCOME_LABEL,
  HOOK_EVENTS,
  allHooksSwitch,
  authoringFor,
  changeFor,
  mcpNameOk,
} from '../src/lib/features/configuration/present';
import { parseInventoryReply } from '../src/lib/contracts/configuration';
import { CHANGE_FAILURES, OUTCOMES } from '../src/lib/contracts/configuration';
import inventory from './fixtures/configuration-inventory-v1.json';
import changes from './fixtures/configuration-changes-v1.json';
import { FakeChanges, fixture } from './configuration-changes-harness';

const damaged = (value: unknown) => structuredClone(value) as any;

describe('what the user can write', () => {
  const item = (over: Record<string, unknown>) =>
    ({
      id: 'a'.repeat(32),
      provider: 'claude_code',
      source: 'claude_user_settings',
      scope: 'global',
      kind: 'hook',
      name: 'x',
      persisted: 'present_enabled',
      effective: 'not_verifiable',
      session_use: 'unknown',
      not_writable: 'not_in_this_phase',
      detail: null,
      hash: null,
      ...over,
    }) as never;
  it('offers a change to a hook of the user’s own settings and a change or a deletion to an OpenCode server, and nothing else', () => {
    expect(authoringFor(item({}))).toEqual({ edit: 'hook', remove: null });
    expect(
      authoringFor(
        item({
          provider: 'opencode',
          source: 'opencode_user_config',
          kind: 'mcp_server',
        }),
      ),
    ).toEqual({ edit: 'mcp', remove: 'park_mcp' });
    expect(
      authoringFor(item({ source: 'claude_project_settings' })),
    ).toBeNull();
    expect(
      authoringFor(item({ provider: 'codex', kind: 'mcp_server' })),
    ).toBeNull();
    expect(authoringFor(item({ not_writable: 'shared_path' }))).toBeNull();
    expect(authoringFor(item({ kind: 'skill' }))).toBeNull();
  });
  it('offers native instruction editing only for supported user files', () => {
    for (const source of [
      'claude_user_rules',
      'codex_user_rules',
      'opencode_user_rules',
    ]) {
      expect(authoringFor(item({ source, kind: 'rule_file' }))).toEqual({
        edit: 'document',
        remove: null,
      });
      expect(
        authoringFor(
          item({ source, kind: 'rule_file', not_writable: 'shared_path' }),
        ),
      ).toBeNull();
    }
    expect(
      authoringFor(item({ source: 'claude_user_skills', kind: 'skill' })),
    ).toEqual({ edit: 'document', remove: null });
    expect(
      authoringFor(
        item({ source: 'codex_user_hooks', kind: 'hook', provider: 'codex' }),
      ),
    ).toBeNull();
    expect(
      authoringFor(item({ source: 'claude_project_rules', kind: 'rule_file' })),
    ).toBeNull();
  });
  it('names events and servers as the desktop checks them', () => {
    expect(HOOK_EVENTS).toContain('PreToolUse');
    expect(HOOK_EVENTS).not.toContain('rm -rf');
    for (const ok of ['docs', 'a_b-1', 'x'.repeat(64)])
      expect(mcpNameOk(ok)).toBe(true);
    for (const bad of ['', 'a b', 'a/b', 'x'.repeat(65), 'a;b', '../x'])
      expect(mcpNameOk(bad)).toBe(false);
  });
});

describe('the change contract', () => {
  it('accepts exactly what the desktop sends', () => {
    expect(fixture.parkPlan().risk).toBe('weakens_security');
    expect(fixture.parkPlan().needs_native_dialog).toBe(true);
    expect(fixture.parkPlan().storage).toBe('sealed');
    expect(fixture.allHooksPlan().operation).toBe('disable_all_hooks');
    expect(fixture.skillPlan().storage).toBeNull();
    expect(fixture.restorePlan().risk).toBe('executes_code');
    const codex = fixture.codexMcpPlan();
    expect(codex.operation).toBe('codex_mcp_off');
    expect(codex.managed_effect).toEqual({
      predicted: true,
      before: 'available',
      after: 'available',
    });
    expect(codex.storage).toBeNull();
    expect(fixture.codexSkillPlan().operation).toBe('codex_skill_on');
    expect(fixture.parkPlan().managed_effect).toBeNull();
    expect(fixture.applied().outcome).toBe('applied');
    expect(fixture.refused().outcome).toBe('refused_by_user');
    expect(fixture.history().changes.length).toBeGreaterThan(1);
    expect(fixture.records().length).toBeGreaterThan(1);
  });

  it.each([
    ['an extra field on a plan', (p: any) => (p.path = '/x')],
    ['a missing field on a plan', (p: any) => delete p.risk],
    ['an unknown operation', (p: any) => (p.operation = 'rm_rf')],
    ['an unknown risk', (p: any) => (p.risk = 'fine')],
    ['a plan id that is not an identifier', (p: any) => (p.id = '../x')],
    ['a diff line of an unknown kind', (p: any) => (p.diff[0].kind = 'run')],
    ['a diff that is not a list', (p: any) => (p.diff = 'x')],
    ['a storage that is not one', (p: any) => (p.storage = 'plain')],
    ['a negative lifetime', (p: any) => (p.expires_in_seconds = -1)],
  ])('refuses %s', (_name, damage) => {
    const plan = damaged(changes.plan_park_blocking);
    damage(plan);
    expect(() => parsePlan(plan)).toThrow(/^configuration: invalid_/);
  });

  it('refuses a result, a history and a record list that do not fit', () => {
    expect(() => parseApplied({ ...changes.applied, outcome: 'ok' })).toThrow();
    expect(() => parseApplied({ ...changes.applied, extra: 1 })).toThrow();
    const history = damaged(changes.history);
    history.changes[0].h0 = 'abc';
    expect(() => parseHistory(history)).toThrow();
    const more = damaged(changes.history);
    more.changes[0].content = 'x';
    expect(() => parseHistory(more)).toThrow();
    const records = damaged(changes.records);
    records[0].state = 'gone';
    expect(() => parseRecords(records)).toThrow();
    expect(() => parseRecords({})).toThrow();
  });

  it('knows every word the desktop sends for an outcome and a failure, and says each in words', () => {
    for (const outcome of OUTCOMES)
      expect(OUTCOME_LABEL[outcome].text.length).toBeGreaterThan(10);
    for (const failure of CHANGE_FAILURES)
      expect(CHANGE_FAILURE_TEXT[failure].length).toBeGreaterThan(10);
  });

  it('reads a refusal as the desktop words it and nothing else', () => {
    expect(parseConfigurationFailure({ change: 'key_lost' })).toBe('key_lost');
    expect(parseConfigurationFailure('invalid')).toBe('invalid');
    expect(parseConfigurationFailure({ change: 'rm -rf' })).toBe('internal');
    expect(parseConfigurationFailure({ change: 'key_lost', x: 1 })).toBe(
      'internal',
    );
    expect(parseConfigurationFailure(['key_lost'])).toBe('internal');
  });

  it('offers a change only for what the desktop can make one to', () => {
    const items = parseInventoryReply(inventory).inventory.items;
    const hook = items.find((i) => i.kind === 'hook')!;
    expect(changeFor(hook)?.operation).toBe('park_hook');
    const codexHook = { ...hook, provider: 'codex' as const };
    expect(changeFor(codexHook)).toBeNull();
    expect(changeFor({ ...hook, not_writable: 'shared_path' })).toBeNull();
    expect(changeFor({ ...hook, persisted: 'present_disabled' })).toBeNull();
    const skill = items.find(
      (i) => i.kind === 'skill' && i.provider === 'claude_code',
    )!;
    expect(changeFor(skill)?.operation).toBe('skill_off');
    expect(
      changeFor({ ...skill, persisted: 'present_disabled' })?.operation,
    ).toBe('skill_on');
    const server = items.find((i) => i.kind === 'mcp_server')!;
    expect(changeFor(server)).toBeNull();
    // Codex: a skill or an MCP server of the user's own configuration, never a hook.
    const codex = (
      kind: 'skill' | 'mcp_server' | 'hook',
      persisted: 'present_enabled' | 'present_disabled',
    ) => ({
      ...hook,
      provider: 'codex' as const,
      source: 'codex_user_config' as const,
      kind,
      persisted,
    });
    expect(changeFor(codex('mcp_server', 'present_enabled'))?.operation).toBe(
      'codex_mcp_off',
    );
    expect(changeFor(codex('mcp_server', 'present_disabled'))?.operation).toBe(
      'codex_mcp_on',
    );
    expect(changeFor(codex('skill', 'present_enabled'))?.operation).toBe(
      'codex_skill_off',
    );
    expect(changeFor(codex('skill', 'present_disabled'))?.operation).toBe(
      'codex_skill_on',
    );
    expect(changeFor(codex('hook', 'present_enabled'))).toBeNull();
    expect(
      changeFor({
        ...codex('skill', 'present_enabled'),
        not_writable: 'shared_path',
      }),
    ).toBeNull();
    expect(
      changeFor({
        ...codex('skill', 'present_enabled'),
        source: 'codex_user_hooks',
      }),
    ).toBeNull();
    expect(changeFor(items.find((i) => i.kind === 'rule_file')!)).toBeNull();
    expect(allHooksSwitch(items)?.operation).toBe('disable_all_hooks');
    expect(
      allHooksSwitch(
        items.map((i) =>
          i.kind === 'hook'
            ? { ...i, persisted: 'present_disabled' as const }
            : i,
        ),
      )?.operation,
    ).toBe('enable_all_hooks');
    expect(allHooksSwitch([])).toBeNull();
  });
});

describe('the changes controller', () => {
  let fake: FakeChanges;
  let states: ChangesState[];
  let refreshed: number;
  let controller: ChangesController;
  const last = () => states[states.length - 1];
  const settle = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  };
  beforeEach(() => {
    vi.useFakeTimers();
    fake = new FakeChanges();
    states = [];
    refreshed = 0;
    controller = new ChangesController(
      (s) => states.push(s),
      () => (refreshed += 1),
      fake.client(),
    );
  });
  afterEach(() => {
    controller.dispose();
    vi.useRealTimers();
  });

  it('asks the desktop to write what the user chose and holds the plan it answers with', async () => {
    controller.author((c) => c.authorHook('Stop', 'Bash', null));
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.calls).toEqual(['author_hook:Stop:Bash:null']);
    expect(states.at(-1)?.plan).not.toBeNull();
    // Nothing was applied by asking.
    expect(fake.calls.some((c) => c.startsWith('apply'))).toBe(false);
    controller.cancel();
    controller.author((c) => c.authorMcp('docs', 'local'));
    controller.cancel();
  });

  it('requests instruction editing by opaque ID without instruction text and never applies automatically', async () => {
    controller.author((c) => c.editDocument('a'.repeat(32)));
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.calls).toEqual(['edit_document:' + 'a'.repeat(32)]);
    expect(states.at(-1)?.plan).not.toBeNull();
  });
  it('treats closing the desktop’s writing window as a cancel, not a failure', async () => {
    fake.failNext = 'cancelled';
    controller.author((c) => c.editHook('a'.repeat(32)));
    await vi.advanceTimersByTimeAsync(0);
    const last = states.at(-1)!;
    expect(last.failure).toBeNull();
    expect(last.notice).toBe('not_done');
    expect(last.plan).toBeNull();
    fake.failNext = 'entry_gone';
    controller.author((c) => c.editMcp('a'.repeat(32)));
    await vi.advanceTimersByTimeAsync(0);
    expect(states.at(-1)?.failure).toBe('entry_gone');
  });

  it('holds a plan and writes nothing until it is confirmed', async () => {
    controller.draft('park_hook', 'a'.repeat(32));
    expect(last().planning).toBe(true);
    await settle();
    expect(last().plan?.operation).toBe('park_hook');
    expect(fake.calls).toEqual([`plan:park_hook:${'a'.repeat(32)}`]);
    expect(refreshed).toBe(0);
  });

  it('applies the plan it holds, once, and reads the inventory again', async () => {
    controller.draft('park_hook', 'a'.repeat(32));
    await settle();
    const id = last().plan!.id;
    controller.apply();
    expect(last().applying).toBe(true);
    controller.apply();
    await settle();
    expect(fake.calls.filter((c) => c.startsWith('apply:'))).toEqual([
      `apply:${id}`,
    ]);
    expect(last().plan).toBeNull();
    expect(last().result).toBe('applied');
    expect(refreshed).toBe(1);
    // Nothing to apply now.
    controller.apply();
    await settle();
    expect(fake.calls.filter((c) => c.startsWith('apply:'))).toHaveLength(1);
  });

  it('does not take a second plan while one is being applied', async () => {
    controller.draft('park_hook', 'a'.repeat(32));
    await settle();
    fake.hold = [];
    controller.apply();
    controller.draft('skill_off', 'b'.repeat(32));
    controller.cancel();
    await settle();
    expect(fake.calls.filter((c) => c.startsWith('plan:'))).toHaveLength(1);
    expect(last().plan).not.toBeNull();
    fake.release();
    await settle();
    expect(last().result).toBe('applied');
  });

  it('says why a plan could not be made, and keeps no plan', async () => {
    fake.failNext = 'key_store_unavailable';
    controller.draft('park_hook', 'a'.repeat(32));
    await settle();
    expect(last().plan).toBeNull();
    expect(last().failure).toBe('key_store_unavailable');
    expect(last().planning).toBe(false);
  });

  it('shows a refusal in the native window as a result and not as an error', async () => {
    fake.applied = fixture.refused();
    controller.draft('park_hook', 'a'.repeat(32));
    await settle();
    controller.apply();
    await settle();
    expect(last().result).toBe('refused_by_user');
    expect(last().failure).toBeNull();
  });

  it('drops a plan the desktop has forgotten', async () => {
    controller.draft('park_hook', 'a'.repeat(32));
    await settle();
    await vi.advanceTimersByTimeAsync(61_000);
    expect(last().plan).toBeNull();
    expect(last().notice).toBe('expired');
  });

  it('reads the history and the kept hooks only when they are shown, and pages the history', async () => {
    expect(fake.calls).toEqual([]);
    controller.show('history');
    await settle();
    expect(fake.calls).toEqual(['history:null']);
    expect(last().history?.changes.length).toBeGreaterThan(1);
    controller.show('parked');
    await settle();
    expect(fake.calls).toContain('records');
    fake.history = {
      changes: fixture.history().changes.slice(0, 1),
      next: null,
    };
    controller.show('history');
    controller.loadHistory(true);
    await settle();
    // No cursor, no further page.
    expect(fake.calls.filter((c) => c.startsWith('history:'))).toHaveLength(1);
  });

  it('asks for the undo of a change as a plan to confirm', async () => {
    controller.undo('c'.repeat(32));
    await settle();
    expect(fake.calls).toEqual([`undo:${'c'.repeat(32)}`]);
    expect(last().plan).not.toBeNull();
    expect(last().result).toBeNull();
  });

  it('exports, imports and forgets one record at a time and says when the user cancelled', async () => {
    controller.show('parked');
    await settle();
    const id = last().records![0].id;
    controller.exportRecord(id);
    controller.importRecord();
    await settle();
    expect(
      fake.calls.filter((c) => c.startsWith('export:') || c === 'import'),
    ).toHaveLength(1);
    expect(last().notice).toBe('exported');
    fake.exported = false;
    controller.exportRecord(id);
    await settle();
    expect(last().notice).toBe('not_done');
    controller.deleteRecord(id);
    await settle();
    expect(fake.calls).toContain(`delete:${id}`);
  });
});
