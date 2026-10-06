import { describe, expect, it } from 'vitest';
import fixtureText from './fixtures/agent-adapter-v1.json?raw';
import {
  AdapterContractError,
  capabilitySupport,
  parseAgentAdapterDescriptor,
  parseManagedAgentSessionObservation,
  parseAgentModelDescriptor,
  parseAdapterError,
  parseAgentAdapterId,
  parseModelProviderId,
  type AgentAdapterId,
  type ModelProviderId,
} from '../src/lib/contracts/agent-adapter';
import {
  parseAgentRuntimeInstanceId,
  parseConsoleSessionId,
  parseTerminalId,
  type ManagedSessionId,
  type AgentRuntimeInstanceId,
} from '../src/lib/contracts/identities';

type Case = { name: string; kind: string; value: unknown; error?: string };
const fixtures = JSON.parse(fixtureText) as { valid: Case[]; invalid: Case[] };
function parse(kind: string, value: unknown) {
  switch (kind) {
    case 'adapter':
      return parseAgentAdapterDescriptor(value);
    case 'session':
      return parseManagedAgentSessionObservation(value);
    case 'model':
      return parseAgentModelDescriptor(value);
    case 'error':
      return parseAdapterError(value);
    default:
      throw new Error('Unknown fixture kind');
  }
}
const value = (name: string) =>
  fixtures.valid.find((c) => c.name === name)!.value;

describe('Agent adapter v1 shared corpus: synthetic, not runtime claims', () => {
  it.each(fixtures.valid)(
    'accepts and semantically round-trips $name',
    ({ kind, value }) => {
      const dto = parse(kind, value);
      expect(JSON.parse(JSON.stringify(dto))).toEqual(value);
      expect(parse(kind, JSON.parse(JSON.stringify(dto)))).toEqual(dto);
    },
  );
  it.each(fixtures.invalid)(
    'rejects with matching Rust diagnostics $name',
    ({ kind, value, error }) => {
      expect(() => parse(kind, value)).toThrow(AdapterContractError);
      expect(() => parse(kind, value)).toThrow(error);
    },
  );
  it('does not silently grant missing capability or access', () => {
    const dto = parseAgentAdapterDescriptor(
      value('codex-like-structured-stdio'),
    );
    expect(capabilitySupport(dto, 'model_selection', 'observe')).toBe(
      'supported',
    );
    expect(capabilitySupport(dto, 'model_selection', 'request')).toBe(
      'unknown',
    );
    expect(capabilitySupport(dto, 'session_resume', 'request')).toBe('unknown');
  });
  it('keeps PTY output opaque without structured claims', () => {
    const dto = parseAgentAdapterDescriptor(value('generic-pty-fallback'));
    expect(capabilitySupport(dto, 'terminal_io', 'request')).toBe('supported');
    expect(capabilitySupport(dto, 'tool_calls', 'observe')).toBe('unsupported');
    expect(capabilitySupport(dto, 'approvals', 'request')).toBe('unknown');
  });
  it('keeps model provider independent from adapter/runtime', () => {
    const dto = parseManagedAgentSessionObservation(value('session-idle'));
    expect(dto.adapter_id).toBe('opencode');
    expect(dto.observed_model?.provider_id).toBe('anthropic');
  });
  it('detaches accepted metadata from the producer', () => {
    const input = structuredClone(
      value('codex-like-structured-stdio'),
    ) as Record<string, unknown>;
    const dto = parseAgentAdapterDescriptor(input);
    input.implementation_version = 'changed';
    expect(dto.implementation_version).toBe('fixture-0.1');
  });
  it.each([undefined, NaN, Infinity, BigInt(1), new Date(), () => 1])(
    'rejects non-JSON extension value %s',
    (bad) => {
      const input = {
        ...parseAgentAdapterDescriptor(value('codex-like-structured-stdio')),
        extensions: { 'vendor.data': { bad } },
      };
      expect(() => parseAgentAdapterDescriptor(input)).toThrow(
        AdapterContractError,
      );
    },
  );
  it('rejects cycles and sparse arrays', () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    for (const bad of [cyclic, { items: Array(2) }]) {
      expect(() =>
        parseAgentAdapterDescriptor({
          ...parseAgentAdapterDescriptor(value('codex-like-structured-stdio')),
          extensions: { 'vendor.data': bad },
        }),
      ).toThrow('invalid_json');
    }
  });
  it('does not let an extension redefine normalized capability support', () => {
    const input = {
      ...parseAgentAdapterDescriptor(value('codex-like-structured-stdio')),
      extensions: { 'codex.capabilities': { session_resume: 'supported' } },
    };
    const dto = parseAgentAdapterDescriptor(input);
    expect(capabilitySupport(dto, 'session_resume', 'request')).toBe('unknown');
    expect(dto.extensions['codex.capabilities']).toEqual({
      session_resume: 'supported',
    });
  });
  it('rejects lone UTF-16 surrogates that cannot become Rust JSON strings', () => {
    for (const extensions of [
      { 'vendor.data': { text: String.fromCharCode(0xd800) } },
      { 'vendor.data': { [String.fromCharCode(0xdfff)]: 'value' } },
    ]) {
      expect(() =>
        parseAgentAdapterDescriptor({
          ...parseAgentAdapterDescriptor(value('codex-like-structured-stdio')),
          extensions,
        }),
      ).toThrow('invalid_json');
    }
  });
  it('preserves static identity separation without claiming UUID text identifies its domain', () => {
    const text = '11111111-1111-4111-8111-111111111111';
    // @ts-expect-error TerminalId is not ManagedSessionId.
    const managed: ManagedSessionId = parseTerminalId(text);
    // @ts-expect-error ConsoleSessionId is not ManagedSessionId.
    const local: ManagedSessionId = parseConsoleSessionId(text);
    // @ts-expect-error Runtime instance identity is not managed session identity.
    const instance: ManagedSessionId = parseAgentRuntimeInstanceId(text);
    // @ts-expect-error ManagedSessionId is not runtime instance identity.
    const runtime: AgentRuntimeInstanceId = managed;
    // @ts-expect-error Model provider is not adapter identity.
    const adapter: AgentAdapterId = parseModelProviderId('anthropic');
    // @ts-expect-error Adapter identity is not model provider.
    const provider: ModelProviderId = parseAgentAdapterId('codex');
    expect([managed, local, instance, runtime]).toEqual([
      text,
      text,
      text,
      text,
    ]);
    expect([adapter, provider]).toEqual(['anthropic', 'codex']);
  });
});
