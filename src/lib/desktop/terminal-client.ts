import { invoke } from '@tauri-apps/api/core';
import type { TerminalId } from '../contracts/identities';
import {
  parseAttach,
  parseHostLifecycle,
  type HostLifecycle,
  parseHostTerminals,
  parseParserBrief,
  type Attach,
  type HostTerminal,
  type TerminalInfo,
  type TerminalOutput,
  type TerminalOutputWire,
  type TerminalSize,
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
  title: string | null = null,
): Promise<TerminalInfo> {
  const info = await invoke<TerminalInfo>('create_terminal', {
    request: { size, cwd, title },
  });
  streams.set(info.terminal_id, { ack: null });
  return info;
}
/** The terminals a host holds, for a window that opens after they were made. */
export async function listHostTerminals(): Promise<HostTerminal[]> {
  try {
    const rows = parseHostTerminals(await invoke<unknown>('host_terminals'));
    for (const row of rows)
      streams.set(row.terminal.terminal_id, { ack: null });
    return rows;
  } catch {
    return [];
  }
}
/** A program that drew its screen before this window attached draws it again. */
export async function redrawTerminal(terminal_id: TerminalId): Promise<void> {
  try {
    await invoke<unknown>('terminal_redraw', { request: { terminal_id } });
  } catch {
    // The terminal may have ended meanwhile: there is nothing to redraw.
  }
}
/**
 * Attaches to a terminal that kept running without this window: the screen its parser kept, to be
 * drawn before reading on (the next read starts where the screen ends). Never starts, restarts or
 * writes to anything. A host without a parser answers `replay`, and an answer that is not exactly
 * an attach answer is an error the caller treats as `replay`.
 */
export async function attachTerminal(terminal_id: TerminalId): Promise<Attach> {
  return parseAttach(
    await invoke<unknown>('terminal_attach', { request: { terminal_id } }),
  );
}
/** What survives what for the terminals the host holds, from facts read on this machine. */
export async function hostLifecycle(): Promise<HostLifecycle> {
  return parseHostLifecycle(await invoke<unknown>('host_persistence'));
}
export const ENABLE_LINGERING = 'enable-lingering';
export const DISABLE_LINGERING = 'disable-lingering';
/**
 * Turns systemd lingering on or off for this account. Only ever called from the button that asks
 * for it, with the confirmation word of that direction: the desktop and the host refuse anything
 * else.
 */
export async function setLingering(enable: boolean): Promise<HostLifecycle> {
  return parseHostLifecycle(
    await invoke<unknown>('host_lingering', {
      request: {
        enable,
        confirm: enable ? ENABLE_LINGERING : DISABLE_LINGERING,
      },
    }),
  );
}
/** The saved layout of tabs and panes, as the desktop kept it; `null` when there is none. */
export async function readLayout(): Promise<unknown> {
  try {
    return await invoke<unknown>('layout_read');
  } catch {
    return null;
  }
}
export async function writeLayout(layout: unknown): Promise<void> {
  if (layout === null) return;
  try {
    await invoke<unknown>('layout_write', { layout });
  } catch {
    // A layout that could not be written is not worth a failed window.
  }
}
/** Whether the terminals live in a host that outlives this window. */
export async function hostIsRemote(): Promise<boolean> {
  try {
    const answer = await invoke<{ remote?: unknown }>('host_info');
    return answer?.remote === true;
  } catch {
    return false;
  }
}

/** A terminal the desktop created for a native session: reading it is registered here. */
export function adoptTerminal(info: TerminalInfo): void {
  streams.set(info.terminal_id, { ack: null });
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
      return {
        ...wire,
        batch_id,
        data: Uint8Array.from(wire.data),
        parser: parseParserBrief(wire.parser),
      };
    })
    .finally(() => {
      stream.pending = undefined;
    });
  return stream.pending;
}
