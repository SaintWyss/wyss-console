import { invoke } from '@tauri-apps/api/core';
import type { TerminalId } from '../contracts/identities';
import type {
  TerminalInfo,
  TerminalOutput,
  TerminalOutputWire,
  TerminalSize,
} from '../contracts/terminal';
import { parseUuid, type Uuid } from '../contracts/uuid';

// Exactly one consumer/read in flight per created local terminal. Credits are
// returned only after the caller has received the previous bounded byte batch.
const streams = new Map<
  TerminalId,
  { ack: Uuid | null; pending?: Promise<TerminalOutput> }
>();
export async function createTerminal(
  size: TerminalSize,
  cwd: string | null = null,
): Promise<TerminalInfo> {
  const info = await invoke<TerminalInfo>('create_terminal', {
    request: { size, cwd },
  });
  streams.set(info.terminal_id, { ack: null });
  return info;
}
export function writeTerminal(
  terminal_id: TerminalId,
  data: Uint8Array,
): Promise<number> {
  if (!data.length || data.length > 4096)
    return Promise.reject(new Error('invalid_input'));
  return invoke<number>('terminal_write', {
    request: { terminal_id, data: Array.from(data) },
  });
}
export function resizeTerminal(
  terminal_id: TerminalId,
  size: TerminalSize,
): Promise<TerminalInfo> {
  return invoke<TerminalInfo>('terminal_resize', {
    request: { terminal_id, size },
  });
}
export function terminalStatus(terminal_id: TerminalId): Promise<TerminalInfo> {
  return invoke<TerminalInfo>('terminal_status', { request: { terminal_id } });
}
export async function closeTerminal(
  terminal_id: TerminalId,
): Promise<TerminalInfo> {
  const info = await invoke<TerminalInfo>('close_terminal', {
    request: { terminal_id },
  });
  streams.delete(terminal_id);
  return info;
}
export function readTerminalOutput(
  terminal_id: TerminalId,
): Promise<TerminalOutput> {
  const stream = streams.get(terminal_id);
  if (!stream) return Promise.reject(new Error('unknown_terminal'));
  if (stream.pending) return stream.pending;
  stream.pending = invoke<TerminalOutputWire>('terminal_read_output', {
    request: { terminal_id, ack: stream.ack },
  })
    .then((wire) => {
      if (
        !Array.isArray(wire.data) ||
        wire.data.length > 16384 ||
        !wire.data.every(
          (byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255,
        ) ||
        wire.data.length > 0 !== (wire.batch_id !== null)
      )
        throw new Error('invalid_output');
      const batch_id = wire.batch_id === null ? null : parseUuid(wire.batch_id);
      stream.ack = batch_id;
      return { ...wire, batch_id, data: Uint8Array.from(wire.data) };
    })
    .finally(() => {
      stream.pending = undefined;
    });
  return stream.pending;
}
