import { parseUuid, type Uuid } from './uuid';

declare const identityBrand: unique symbol;
type Identity<Name extends string> = Uuid & { readonly [identityBrand]: Name };
export type ConsoleSessionId = Identity<'ConsoleSessionId'>;
export type TerminalId = Identity<'TerminalId'>;
export type AgentRuntimeInstanceId = Identity<'AgentRuntimeInstanceId'>;
export type ManagedSessionId = Identity<'ManagedSessionId'>;
export type ManagedTurnId = Identity<'ManagedTurnId'>;

export function parseConsoleSessionId(value: unknown): ConsoleSessionId {
  return parseUuid(value) as ConsoleSessionId;
}
export function parseTerminalId(value: unknown): TerminalId {
  return parseUuid(value) as TerminalId;
}
export function parseManagedSessionId(value: unknown): ManagedSessionId {
  return parseUuid(value) as ManagedSessionId;
}
export function parseManagedTurnId(value: unknown): ManagedTurnId {
  return parseUuid(value) as ManagedTurnId;
}

export function parseAgentRuntimeInstanceId(
  value: unknown,
): AgentRuntimeInstanceId {
  return parseUuid(value) as AgentRuntimeInstanceId;
}
