import { describe, expect, it } from 'vitest';
import fixtureText from './fixtures/event-envelope-v1.json?raw';
import type fixtureShape from './fixtures/event-envelope-v1.json';
import {
  ContractValidationError,
  DEFAULT_PERSISTENCE,
  parseEventEnvelope,
  parseSequence,
} from '../src/lib/contracts/event-envelope';
import {
  parseConsoleSessionId,
  parseManagedSessionId,
  parseTerminalId,
  type ConsoleSessionId,
  type ManagedSessionId,
} from '../src/lib/contracts/identities';

// Read JSON text directly so keys such as __proto__ keep JSON object semantics.
const fixtures = JSON.parse(fixtureText) as typeof fixtureShape;

describe('EventEnvelope v1 shared wire corpus', () => {
  it.each(fixtures.valid)(
    'accepts and semantically round-trips: $name',
    ({ event }) => {
      const validated = parseEventEnvelope(event);
      const serialized: unknown = JSON.parse(JSON.stringify(validated));
      expect(serialized).toEqual(event);
      expect(parseEventEnvelope(serialized)).toEqual(validated);
      expect(typeof validated.sequence).toBe('string');
    },
  );
  it.each(fixtures.invalid)(
    'rejects with deterministic diagnostics: $name',
    ({ event, error }) => {
      expect(() => parseEventEnvelope(event)).toThrow(ContractValidationError);
      expect(() => parseEventEnvelope(event)).toThrow(
        `${error.field}: ${error.code}`,
      );
      try {
        parseEventEnvelope(event);
      } catch (actual) {
        expect(actual).toMatchObject(error);
      }
    },
  );
  it('defaults conceptually to ephemeral without filling missing wire fields', () => {
    expect(DEFAULT_PERSISTENCE).toBe('ephemeral');
    const missing: Record<string, unknown> = { ...fixtures.valid[0].event };
    delete missing.persistence;
    expect(() => parseEventEnvelope(missing)).toThrow(
      'persistence: missing_field',
    );
  });
  it('preserves max-u64 without converting canonical sequence to number', () => {
    expect(parseSequence('18446744073709551615')).toBe('18446744073709551615');
    expect(() => parseSequence(Number('18446744073709551615'))).toThrow();
  });
  it('detaches validated payload from external input', () => {
    const input = {
      ...fixtures.valid[0].event,
      payload: { nested: { count: 1 } },
    };
    const event = parseEventEnvelope(input);
    input.payload.nested.count = 2;
    expect(event.payload).toEqual({ nested: { count: 1 } });
  });
  it.each([undefined, NaN, Infinity, BigInt(1), () => 1, new Date()])(
    'rejects non-JSON runtime payload value %s',
    (value) => {
      expect(() =>
        parseEventEnvelope({ ...fixtures.valid[0].event, payload: { value } }),
      ).toThrow();
    },
  );
  it('rejects cycles and sparse arrays rather than changing their meaning', () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    for (const payload of [cyclic, { sparse: Array(1) }]) {
      expect(() =>
        parseEventEnvelope({ ...fixtures.valid[0].event, payload }),
      ).toThrow('payload: invalid_json_value');
    }
  });
  it('does not claim to detect secrets from content or redaction labels', () => {
    const input = {
      ...fixtures.valid[0].event,
      persistence: 'durable',
      redaction: 'applied',
      payload: { text: 'synthetic uninspected content' },
    };
    expect(parseEventEnvelope(input).payload).toEqual(input.payload);
  });
});

describe('local identities', () => {
  it('validates UUIDs while keeping local and canonical types distinct', () => {
    const value = 'ABCDEFAB-1234-4567-89AB-ABCDEFABCDEF';
    expect(parseConsoleSessionId(value)).toBe(value);
    expect(parseTerminalId(value)).toBe(value);
    expect(parseManagedSessionId(value)).toBe(value);
    expect(() => parseTerminalId('invalid')).toThrow();
    expect(() => parseConsoleSessionId('invalid')).toThrow();
    // @ts-expect-error TerminalId must not be assigned to ConsoleSessionId.
    const local: ConsoleSessionId = parseTerminalId(value);
    // @ts-expect-error ConsoleSessionId must not be assigned to ManagedSessionId.
    const managed: ManagedSessionId = parseConsoleSessionId(value);
    expect([local, managed]).toEqual([value, value]);
  });
});
