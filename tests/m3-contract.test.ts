import { expect, it } from 'vitest';
import { parseAck, parseM3State } from '../src/lib/contracts/m3';
import inactive from './fixtures/m3/state-inactive.json';
import inference from './fixtures/m3/state-fixture-inference.json';
import held from './fixtures/m3/state-fixture-tools-held.json';
import ended from './fixtures/m3/state-fixture-tools-ended.json';
import refused from './fixtures/m3/state-fixture-refused.json';
import failed from './fixtures/m3/state-fixture-failed.json';

const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v));
type Any = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

it('decodes every payload the Rust projection produces', () => {
  for (const file of [inactive, inference, held, ended, refused, failed]) {
    expect(() => parseM3State(file)).not.toThrow();
  }
});

it('keeps what the provider did not report as null, never as zero', () => {
  const s = parseM3State(inference);
  const turns = s.fixture!.report!.usage.turns;
  expect(turns).toHaveLength(2);
  // The first turn never got a usage report.
  expect(turns[0]!.total).toEqual({
    input: null,
    output: null,
    cache_creation: null,
    cache_read: null,
    reasoning: null,
  });
  expect(turns[0]!.completeness).toBe('unreported');
  expect(turns[1]!.total.input).toBe(11);
  expect(turns[1]!.estimate_usd).not.toBeNull();
});

it('carries the four model facts apart, and an alias is not an id', () => {
  const m = parseM3State(inference).fixture!.report!.model!;
  expect(m.offered!.entitlement).toBe('not_checked');
  expect(m.offered!.entries.map((e) => e.model_id)).toContain(
    'claude-haiku-5-5',
  );
  expect(m.requested!.resolved_model).toBe('claude-sonnet-5-5');
  expect(m.reported_in_use).toEqual(['claude-haiku-5-5']);
  expect(m.reroute.state).toBe('rerouted');
  expect(m.configured).toBeNull();
});

it('keeps every state of a tool call distinct', () => {
  const f = parseM3State(ended).fixture!;
  expect(f.approvals.map((a) => [a.item, a.state])).toEqual([
    ['claude-act-1', 'confirmed'],
    ['claude-act-2', 'declined'],
    ['claude-act-3', 'failed'],
    ['claude-act-4', 'indeterminate'],
    ['claude-act-6', 'pending'],
  ]);
  const r = f.report!.actions.find((a) => a.item === 'claude-act-5');
  expect(r).toMatchObject({ state: 'refused', refusal: 'out_of_scope' });
  // Only the pending one offers decisions.
  expect(f.approvals.filter((a) => a.decisions.length > 0)).toHaveLength(1);
});

it('refuses a payload with an extra or missing key at any level', () => {
  const paths: string[][] = [
    [],
    ['profiles', '0'],
    ['profiles', '0', 'evidence'],
    ['model_selection'],
    ['history'],
    ['fixture'],
    ['fixture', 'approvals', '0'],
    ['fixture', 'report'],
    ['fixture', 'report', 'usage'],
    ['fixture', 'report', 'usage', 'running', 'input'],
    ['fixture', 'report', 'usage', 'turns', '0'],
    ['fixture', 'report', 'usage', 'turns', '0', 'total'],
    ['fixture', 'report', 'model'],
    ['fixture', 'report', 'model', 'offered'],
    ['fixture', 'report', 'tap'],
    ['fixture', 'report', 'actions', '0'],
  ];
  for (const path of paths) {
    for (const mutate of ['extra', 'missing'] as const) {
      const v = copy(inference) as Any;
      const w = copy(ended) as Any;
      for (const doc of [v, w]) {
        let node: Any = doc;
        let ok = true;
        for (const k of path) {
          if (node?.[k] === undefined) {
            ok = false;
            break;
          }
          node = node[k];
        }
        if (!ok || typeof node !== 'object' || node === null) continue;
        if (mutate === 'extra') node.surprise = 1;
        else delete node[Object.keys(node)[0]!];
        expect(
          () => parseM3State(doc),
          `${path.join('.')} ${mutate}`,
        ).toThrow();
        // restore for the next doc
        if (mutate === 'extra') delete node.surprise;
      }
    }
  }
});

it('refuses unknown words and out-of-range numbers', () => {
  const bad = (edit: (v: Any) => void) => {
    const v = copy(inference) as Any;
    edit(v);
    expect(() => parseM3State(v)).toThrow();
  };
  bad((v) => (v.profiles[2].availability = 'ready'));
  bad((v) => (v.profiles[2].provider = 'gemini'));
  bad((v) => (v.fixture.phase = 'paused'));
  bad((v) => (v.fixture.report.usage.turns[0].total.input = -1));
  bad((v) => (v.fixture.report.usage.turns[0].total.input = 1.5));
  bad((v) => (v.fixture.report.usage.turns[0].total.input = '11'));
  bad((v) => (v.fixture.report.usage.turns[0].completeness = 'done'));
  bad((v) => (v.fixture.report.usage.limits[0].utilization = 1.2));
  bad((v) => (v.fixture.report.usage.limits[0].window = 'monthly'));
  bad((v) => (v.fixture.report.model.offered.entitlement = 'checked'));
  bad((v) => (v.fixture.report.model.reroute.state = 'swapped'));
  bad(
    (v) =>
      (v.fixture.report.model.effort = { state: 'effective', value: 'low' }),
  );
  bad((v) => (v.version = 'm3.2'));
  bad((v) => (v.capability = 'm4'));
  bad((v) => (v.history.retention_days = -30));
});

it('refuses a start action on a profile that is not available', () => {
  const v = copy(inactive) as Any;
  v.profiles[2].can_start = true;
  expect(() => parseM3State(v)).toThrow();
  const w = copy(inactive) as Any;
  w.profiles[0].availability = 'available';
  expect(() => parseM3State(w)).toThrow();
});

it('requires all four profiles, once each, in order', () => {
  for (const edit of [
    (v: Any) => v.profiles.pop(),
    (v: Any) => v.profiles.reverse(),
    (v: Any) => v.profiles.push(copy(v.profiles[0])),
    (v: Any) => (v.profiles[1] = copy(v.profiles[0])),
  ]) {
    const v = copy(inactive) as Any;
    edit(v);
    expect(() => parseM3State(v)).toThrow();
  }
});

it('refuses oversized collections and text', () => {
  const v = copy(inference) as Any;
  v.fixture.conversation = Array.from({ length: 201 }, () => ({
    role: 'user',
    text: 'x',
  }));
  expect(() => parseM3State(v)).toThrow();
  const w = copy(inference) as Any;
  w.fixture.detail = 'x'.repeat(161);
  expect(() => parseM3State(w)).toThrow();
});

it('tells the inactive state: Claude is blocked and nothing can start', () => {
  const s = parseM3State(inactive);
  expect(s.profiles.map((p) => p.can_start)).toEqual([
    false,
    false,
    false,
    false,
  ]);
  expect(s.profiles[2]!.availability).toBe('blocked');
  expect(s.claude_install.state).toBe('not_configured');
  expect(s.model_selection.available).toBe(false);
  expect(s.history).toMatchObject({
    state: 'off',
    enabled: false,
    can_enable: false,
  });
  expect(s.fixture).toBeNull();
});

it('decodes an acknowledgement strictly', () => {
  expect(parseAck({ ok: true, error: null })).toEqual({
    ok: true,
    error: null,
  });
  expect(parseAck({ ok: false, error: 'not_pending' }).error).toBe(
    'not_pending',
  );
  expect(() => parseAck({ ok: true })).toThrow();
  expect(() => parseAck({ ok: 'yes', error: null })).toThrow();
  expect(() => parseAck({ ok: false, error: 'has space' })).toThrow();
});
