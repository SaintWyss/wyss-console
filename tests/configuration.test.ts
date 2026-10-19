/* eslint-disable @typescript-eslint/no-explicit-any -- damage is done to arbitrary JSON */
import { describe, expect, it } from 'vitest';
import {
  parseChanges,
  parseConfigurationFailure,
  parseInventoryReply,
} from '../src/lib/contracts/configuration';
import {
  detailLines,
  failureText,
  kindCounts,
  mayHoldSecret,
  outcomeText,
  visibleItems,
} from '../src/lib/features/configuration/present';
import fixture from './fixtures/configuration-inventory-v1.json';

const reply = () => structuredClone(fixture) as Record<string, any>;

describe('the inventory contract', () => {
  it('accepts exactly what the desktop sends', () => {
    const parsed = parseInventoryReply(fixture);
    expect(parsed.revision).toBe(1);
    expect(parsed.inventory.items).toHaveLength(13);
    expect(parsed.inventory.sources).toHaveLength(17);
    // Every variant of an outcome is in the fixture, and every one is believed.
    expect(new Set(parsed.inventory.sources.map((s) => s.outcome))).toEqual(
      new Set(['read', 'absent', 'failed', 'project_not_trusted']),
    );
  });

  it.each([
    ['an extra field on the reply', (r: any) => (r.extra = 1)],
    [
      'an extra field on an item',
      (r: any) => (r.inventory.items[0].path = '/x'),
    ],
    [
      'a missing field on an item',
      (r: any) => delete r.inventory.items[0].hash,
    ],
    ['an unknown kind', (r: any) => (r.inventory.items[0].kind = 'extension')],
    ['an unknown provider', (r: any) => (r.inventory.items[0].provider = 'x')],
    ['an unknown source', (r: any) => (r.inventory.sources[0].key = 'home')],
    ['a negative revision', (r: any) => (r.revision = -1)],
    ['a fractional revision', (r: any) => (r.revision = 1.5)],
    [
      'a hash that is not a digest',
      (r: any) => (r.inventory.sources[1].hash = 'abc'),
    ],
    [
      'a flag that is not a boolean',
      (r: any) => (r.inventory.truncated = 'no'),
    ],
    ['an item list that is not a list', (r: any) => (r.inventory.items = {})],
    [
      'a projection of an unknown kind',
      (r: any) =>
        (r.inventory.items[0].detail = { kind: 'html', value: '<b>' }),
    ],
    [
      'a secret class that is not one',
      (r: any) => (r.inventory.items[2].detail.value[2][1].value = 'password'),
    ],
    [
      'a source that is read and failed at once',
      (r: any) => (r.inventory.sources[1].failure = { failure: 'missing' }),
    ],
  ])('refuses %s', (_name, damage) => {
    const damaged = reply();
    damage(damaged);
    expect(() => parseInventoryReply(damaged)).toThrow(
      /^configuration: invalid_/,
    );
  });

  it('says a source has a conflict copy beside it, and refuses a reading that omits it', () => {
    const parsed = parseInventoryReply(fixture).inventory;
    const read = parsed.sources.find((s) => s.outcome === 'read')!;
    expect(
      outcomeText({ ...read, conflict_copy: true } as typeof read),
    ).toContain('A conflict copy of this file sits beside it');
    expect(outcomeText(read)).not.toContain('conflict copy');
    const damaged = reply();
    delete damaged.inventory.sources.find((s: any) => s.outcome === 'read')
      .conflict_copy;
    expect(() => parseInventoryReply(damaged)).toThrow();
  });

  it('refuses a projection nested deeper than any the desktop sends', () => {
    let deep: unknown = { kind: 'null' };
    for (let i = 0; i < 60; i++) deep = { kind: 'list', value: [deep] };
    const damaged = reply();
    damaged.inventory.items[0].detail = deep;
    expect(() => parseInventoryReply(damaged)).toThrow();
  });

  it('refuses anything that is not an object', () => {
    for (const value of [null, 1, 'x', [], undefined])
      expect(() => parseInventoryReply(value)).toThrow();
  });

  it('reads the three answers about changes and nothing else', () => {
    expect(parseChanges({ result: 'same', revision: 3 })).toEqual({
      result: 'same',
      revision: 3,
    });
    expect(
      parseChanges({
        result: 'changed',
        revision: 4,
        sources: ['codex_user_config'],
      }),
    ).toEqual({
      result: 'changed',
      revision: 4,
      sources: ['codex_user_config'],
    });
    expect(parseChanges({ result: 'resync', revision: 9 }).result).toBe(
      'resync',
    );
    expect(() => parseChanges({ result: 'changed', revision: 4 })).toThrow();
    expect(() =>
      parseChanges({ result: 'same', revision: 4, sources: [] }),
    ).toThrow();
    expect(() => parseChanges({ result: 'maybe', revision: 4 })).toThrow();
    expect(() =>
      parseChanges({
        result: 'changed',
        revision: 4,
        sources: ['/etc/passwd'],
      }),
    ).toThrow();
  });

  it('knows only one failure the desktop names', () => {
    expect(parseConfigurationFailure('unavailable')).toBe('unavailable');
    expect(parseConfigurationFailure('/home/x/secret')).toBe('internal');
    expect(parseConfigurationFailure(undefined)).toBe('internal');
  });
});

describe('presentation', () => {
  const parsed = parseInventoryReply(fixture).inventory;

  it('never shows a secret: what was hidden says what it was, not what it held', () => {
    const server = parsed.items.find((i) => i.kind === 'mcp_server')!;
    const lines = detailLines(server.detail);
    const key = lines.find((l) => l.name === 'env.CONTEXT7_API_KEY')!;
    expect(key).toEqual({
      name: 'env.CONTEXT7_API_KEY',
      value: 'hidden (environment value)',
      hidden: true,
    });
    expect(JSON.stringify(parsed)).not.toContain('SEKRET');
    expect(mayHoldSecret(server.detail)).toBe(true);
  });

  it('marks a redacted command and leaves a plain one unmarked', () => {
    const hook = parsed.items.find((i) => i.kind === 'hook')!;
    const command = detailLines(hook.detail).find((l) => l.name === 'command')!;
    expect(command.value).toBe('guard.sh --token [redacted]');
    expect(command.hidden).toBe(true);
    const model = parsed.items.find((i) => i.kind === 'model_preference')!;
    expect(mayHoldSecret(model.detail)).toBe(false);
  });

  it('flattens nested values into dotted names in order', () => {
    expect(
      detailLines({
        kind: 'map',
        value: [
          [
            'a',
            {
              kind: 'list',
              value: [{ kind: 'bool', value: true }, { kind: 'null' }],
            },
          ],
          ['b', { kind: 'map', value: [['c', { kind: 'number', value: 2 }]] }],
          ['empty', { kind: 'list', value: [] }],
        ],
      }).map((l) => [l.name, l.value]),
    ).toEqual([
      ['a[0]', 'true'],
      ['a[1]', 'empty'],
      ['b.c', '2'],
      ['empty', 'none'],
    ]);
    expect(detailLines(null)).toEqual([]);
  });

  it('filters by agent and kind and counts what a filter leaves', () => {
    const all = parsed.items;
    expect(visibleItems(all, 'all', 'all')).toHaveLength(all.length);
    const codex = visibleItems(all, 'codex', 'all');
    expect(codex.every((i) => i.provider === 'codex')).toBe(true);
    expect(
      visibleItems(all, 'all', 'skill').every((i) => i.kind === 'skill'),
    ).toBe(true);
    const counts = kindCounts(all, 'codex');
    expect(counts.all).toBe(codex.length);
    expect(
      counts.hook +
        counts.skill +
        counts.mcp_server +
        counts.rule_file +
        counts.model_preference,
    ).toBe(counts.all);
  });

  it('says what became of every source without a path or an operating-system text', () => {
    for (const source of parsed.sources) {
      const said = outcomeText(source);
      expect(said.length).toBeGreaterThan(3);
      expect(said).not.toMatch(/\/|errno|permission denied|No such file/i);
    }
    expect(
      outcomeText(parsed.sources.find((s) => s.outcome === 'failed')!),
    ).toContain('line 1, column 3');
    expect(
      failureText({ failure: 'untrusted_path', reason: 'outside_roots' }),
    ).toContain('outside');
    expect(failureText({ failure: 'credential_file' })).toContain(
      'never opened',
    );
  });
});
