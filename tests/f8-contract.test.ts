import { describe, expect, it } from 'vitest';
import {
  parseAbandonReceipt,
  parseAuditInfo,
  parseBindingReceipt,
  parseDecisionReceipt,
  parseF8Failure,
  parseF8State,
  parseLeaseReceipt,
  parseModelsInfo,
  parseRuntimeReceipt,
  parseSessionDetail,
  parseSessionReceipt,
  parseSessionsView,
  parseTurnReading,
  parseTurnReceipt,
  parseWatchInfo,
} from '../src/lib/contracts/f8';
import { parseApprovalDetail } from '../src/lib/contracts/approvals';
import fixture from './fixtures/f8-gui-v1.json';
import type { Raw } from './f8-harness';

// The fixture is written by the Rust test that drives a real application through the
// window's service, and that test fails when a payload drifts from it. These tests are the
// other half: the window's parsers accept what the desktop really sends and refuse the rest.
const fx = fixture as Record<string, Raw>;
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));

describe('what the desktop really sends', () => {
  it('parses every payload of the real application', () => {
    expect(parseF8State(fx.state).authority?.protocol_version).toBe(2);
    expect(parseF8State(fx.state_inactive).active).toBe(false);
    expect(parseSessionsView(fx.sessions).sessions.length).toBeGreaterThan(0);
    expect(parseSessionDetail(fx.session_stopped).session.runtime.actual).toBe(
      'stopped',
    );
    const ready = parseSessionDetail(fx.session_ready);
    expect(ready.session.runtime.profile).toBe('codex_local_tools_v1');
    expect(ready.session.runtime.model_count).toBeGreaterThan(0);
    const attached = parseSessionDetail(fx.session_attached);
    expect(attached.binding.state).toBe('bound_attached');
    expect(attached.lease?.scopes).toHaveLength(5);
    expect(parseSessionReceipt(fx.session_receipt).id).toBeTruthy();
    expect(parseRuntimeReceipt(fx.runtime_receipt).generation).toBe(1);
    expect(parseLeaseReceipt(fx.lease_receipt).lease.state).toBe('active');
    expect(parseBindingReceipt(fx.binding_receipt).workspace).toMatch(/^\//);
    expect(parseTurnReceipt(fx.turn_receipt).id).toBeTruthy();
    const reading = parseTurnReading(fx.turn_reading);
    expect(reading.approval?.kind).toBe('command_execution');
    expect(reading.events[0]?.kind).toBe('text');
    const approval = parseApprovalDetail(fx.approval);
    expect(approval.approval.decidable).toBe(true);
    expect(approval.command?.command).toBeTruthy();
    expect(parseDecisionReceipt(fx.decision_receipt).state).toBe('accepted');
    const models = parseModelsInfo(fx.models);
    expect(models.entries[0]?.efforts).toEqual(['medium']);
    expect(parseAuditInfo(fx.audit).page.events.length).toBeGreaterThan(0);
    expect(parseWatchInfo(fx.watch).changed).toBe(true);
    expect(parseF8Failure(fx.failure).code).toBe('stale');
  });
  it('carries one authority instance across every payload of one run', () => {
    const named = [
      fx.sessions,
      fx.session_ready,
      fx.session_attached,
      fx.turn_reading,
      fx.models,
      fx.audit,
    ].map((p) => p.authority);
    expect(new Set(named).size).toBe(1);
    expect(fx.state.authority.instance_id).toBe(named[0]);
  });
});

describe('what it refuses', () => {
  it('refuses any key that is not in the projection, and any that is missing', () => {
    for (const [name, parse] of [
      ['state', parseF8State],
      ['sessions', parseSessionsView],
      ['session_attached', parseSessionDetail],
      ['turn_reading', parseTurnReading],
      ['models', parseModelsInfo],
      ['audit', parseAuditInfo],
      ['watch', parseWatchInfo],
      ['lease_receipt', parseLeaseReceipt],
      ['decision_receipt', parseDecisionReceipt],
      ['binding_receipt', parseBindingReceipt],
      ['runtime_receipt', parseRuntimeReceipt],
    ] as const) {
      const extra = copy(fx[name]);
      extra.process_id = 7;
      expect(() => parse(extra), `${name} +key`).toThrow();
      const missing = copy(fx[name]);
      delete missing.authority;
      expect(() => parse(missing), `${name} -key`).toThrow();
      const other = copy(fx[name]);
      other.schema_version = 2;
      expect(() => parse(other), `${name} schema`).toThrow();
    }
    expect(() => parseAbandonReceipt({})).toThrow();
  });

  it('refuses a runtime that contradicts itself', () => {
    const lies: ((d: Raw) => void)[] = [
      // Ready without an identity.
      (d) => (d.session.runtime.instance = null),
      // A fence of another generation than the row carries.
      (d) => (d.session.runtime.fence.generation = 9),
      // A fence of another session.
      (d) => (d.session.runtime.fence.session = d.authority),
      // Local tools without the folder it is isolated for.
      (d) => (d.session.runtime.workspace = null),
      // Models listed by a runtime that is not ready.
      (d) => {
        d.session.runtime.actual = 'starting';
        d.session.runtime.model_count = 3;
        d.session.runtime.instance = null;
        d.session.runtime.fence.instance = null;
        d.session.runtime.version = null;
      },
      // A session revision that is neither prepared's nor closed's.
      (d) => (d.session.revision = 5),
      // A workspace that is not absolute.
      (d) => (d.binding.workspace = 'relative/dir'),
      // An attachment without an instance.
      (d) => (d.binding.attached_instance = null),
      // A model reported by a session that is not attached.
      (d) => {
        d.binding.state = 'bound_detached';
        d.binding.mode = null;
        d.binding.attached_instance = null;
        d.binding.observed_model = {
          model_id: 'x',
          provider_id: null,
          reasoning_effort: null,
          source: 'thread_start',
        };
      },
      // A lease that ends before it begins.
      (d) => (d.lease.expires_at = d.lease.issued_at),
      // A revoked lease that does not say why.
      (d) => (d.lease.state = 'revoked'),
      // A scope that does not exist.
      (d) => (d.lease.scopes = ['root']),
      // The same scope twice.
      (d) => (d.lease.scopes = ['turn_start', 'turn_start']),
      // A gate reason that is not the authority's.
      (d) => (d.gates[0].reason = 'because'),
    ];
    for (const [index, lie] of lies.entries()) {
      const detail = copy(fx.session_attached);
      lie(detail);
      expect(() => parseSessionDetail(detail), `lie ${index}`).toThrow();
    }
  });

  it('refuses a fence with a generation that is not a count', () => {
    for (const bad of [-1, 1.5, '1', null, Number.MAX_SAFE_INTEGER + 2]) {
      const detail = copy(fx.session_ready);
      detail.session.runtime.generation = bad;
      expect(() => parseSessionDetail(detail), String(bad)).toThrow();
    }
  });

  it('refuses a session list that is out of order, repeats a session or points the wrong way', () => {
    const view = copy(fx.sessions);
    view.sessions.push(copy(view.sessions[0]));
    expect(() => parseSessionsView(view)).toThrow();
    const older = copy(fx.sessions);
    older.older = '999';
    expect(() => parseSessionsView(older)).toThrow();
  });

  it('refuses a reading that skips backwards or whose approval outlives its turn', () => {
    const backwards = copy(fx.turn_reading);
    backwards.events.push({ kind: 'text', sequence: '1', text: 'again' });
    expect(() => parseTurnReading(backwards)).toThrow();
    const ended = copy(fx.turn_reading);
    ended.turn.turn.state = 'completed';
    ended.turn.turn.cancellable = false;
    ended.turn.turn.ended_at = ended.turn.turn.updated_at;
    ended.turn.turn.reason = 'completed';
    expect(() => parseTurnReading(ended)).toThrow();
    const cursor = copy(fx.turn_reading);
    cursor.next = '7';
    expect(() => parseTurnReading(cursor)).toThrow();
  });

  it('refuses a decision receipt that is still pending, and models without a launch', () => {
    const receipt = copy(fx.decision_receipt);
    receipt.state = 'pending';
    expect(() => parseDecisionReceipt(receipt)).toThrow();
    const models = copy(fx.models);
    models.fence.instance = null;
    expect(() => parseModelsInfo(models)).toThrow();
    const twice = copy(fx.models);
    twice.entries.push(copy(twice.entries[0]));
    expect(() => parseModelsInfo(twice)).toThrow();
  });

  it('refuses an inactive state that shows a connection', () => {
    const state = copy(fx.state_inactive);
    state.link.state = 'connected';
    expect(() => parseF8State(state)).toThrow();
    const awake = copy(fx.state);
    awake.link.state = 'waiting';
    expect(() => parseF8State(awake)).toThrow();
  });

  it('turns anything that is not a failure into an internal one', () => {
    for (const bad of [
      null,
      'x',
      {},
      { code: 'nope', retry: false, retry_in_ms: null },
    ]) {
      expect(parseF8Failure(bad)).toEqual({
        code: 'internal',
        retry: false,
        retry_in_ms: null,
      });
    }
    expect(
      parseF8Failure({ code: 'unavailable', retry: true, retry_in_ms: 250 }),
    ).toEqual({ code: 'unavailable', retry: true, retry_in_ms: 250 });
  });
});
