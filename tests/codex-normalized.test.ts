import { describe, expect, it } from 'vitest';
import corpusText from './fixtures/codex-app-server-v1.json?raw';
import descriptorText from './fixtures/codex-offline-descriptor-v1.json?raw';
import {
  parseManagedAgentSessionObservation,
  parseAgentModelDescriptor,
  parseAdapterError,
  parseAgentAdapterDescriptor,
  capabilitySupport,
} from '../src/lib/contracts/agent-adapter';

type Fixture = {
  name: string;
  wire: string;
  expected?: {
    common?: unknown;
    error?: unknown;
    models?: { model: unknown }[];
  };
};
const corpus = JSON.parse(corpusText) as {
  valid: Fixture[];
  invalid: Fixture[];
};
const projections = corpus.valid.flatMap(({ name, expected }) => [
  ...(expected?.common
    ? [{ name: `${name}:session`, kind: 'session', value: expected.common }]
    : []),
  ...(expected?.error
    ? [{ name: `${name}:error`, kind: 'error', value: expected.error }]
    : []),
  ...(expected?.models?.map(({ model }, i) => ({
    name: `${name}:model-${i}`,
    kind: 'model',
    value: model,
  })) ?? []),
]);

describe('Codex offline projections: same expected outputs checked against Rust normalization', () => {
  it.each(projections)('accepts and round-trips $name', ({ kind, value }) => {
    const dto =
      kind === 'session'
        ? parseManagedAgentSessionObservation(value)
        : kind === 'model'
          ? parseAgentModelDescriptor(value)
          : parseAdapterError(value);
    expect(JSON.parse(JSON.stringify(dto))).toEqual(value);
  });
  it('keeps raw wire text opaque to JavaScript, including signed i64 extrema', () => {
    expect(
      corpus.valid.find((c) => c.name === 'request-max-i64')?.wire,
    ).toContain('9223372036854775807');
    expect(
      corpus.valid.find((c) => c.name === 'request-min-i64')?.wire,
    ).toContain('-9223372036854775808');
    expect(corpus.valid).toHaveLength(75);
    expect(corpus.invalid).toHaveLength(36);
    expect(projections).toHaveLength(18);
    expect(
      [...corpus.valid, ...corpus.invalid].every(
        (c) => typeof c.wire === 'string',
      ),
    ).toBe(true);
  });
  it('does not invent model entitlement or provider from the Codex runtime name', () => {
    const model = parseAgentModelDescriptor(
      corpus.valid.find((c) => c.name === 'model-list-response')!.expected!
        .models![0]!.model,
    );
    expect(model.provider_id).toBeNull();
    expect(model.availability).toBe('unknown');
  });
  it('makes static translation coverage conditional and local compatibility unknown', () => {
    const descriptor = parseAgentAdapterDescriptor(JSON.parse(descriptorText));
    expect(descriptor.compatibility.state).toBe('unknown');
    expect(descriptor.runtime_version.value).toBeNull();
    expect(capabilitySupport(descriptor, 'model_list', 'observe')).toBe(
      'conditional',
    );
    expect(capabilitySupport(descriptor, 'subagents', 'request')).toBe(
      'unknown',
    );
    expect(JSON.stringify(descriptor)).not.toContain('confirmed');
  });
});
