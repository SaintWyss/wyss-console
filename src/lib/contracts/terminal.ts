import type { TerminalId } from './identities';
import type { Uuid } from './uuid';

export interface TerminalSize {
  rows: number;
  cols: number;
}
export type TerminalState =
  'starting' | 'running' | 'exited' | 'closing' | 'closed' | 'failed';
export interface TerminalInfo {
  terminal_id: TerminalId;
  shell: string;
  cwd: string;
  observed_pid: number | null;
  size: TerminalSize;
  state: TerminalState;
  exit: { code: number; signal: string | null } | null;
  error: string | null;
}
export interface TerminalOutput {
  batch_id: Uuid | null;
  data: Uint8Array;
  eof: boolean;
  truncated: boolean;
  status: TerminalInfo;
}
export interface TerminalOutputWire extends Omit<TerminalOutput, 'data'> {
  data: number[];
}

/** What a native session's tab says about it (from the host). */
export interface HostNativeTag {
  session_id: string;
  agent_name: string;
  project: string;
  profile: string | null;
  through_shell: boolean;
}
/** A terminal a host holds: what a window that opens later shows again. */
export interface HostTerminal {
  terminal: TerminalInfo;
  title: string;
  created_at: number;
  native: HostNativeTag | null;
}

/** The host's list, or nothing when the answer is not a list of terminals (no host). */
export function parseHostTerminals(value: unknown): HostTerminal[] {
  if (!Array.isArray(value)) return [];
  const out: HostTerminal[] = [];
  for (const row of value) {
    if (typeof row !== 'object' || row === null) continue;
    const r = row as Record<string, unknown>;
    const t = r.terminal as Record<string, unknown> | undefined;
    if (
      !t ||
      typeof t.terminal_id !== 'string' ||
      typeof r.title !== 'string' ||
      typeof r.created_at !== 'number'
    )
      continue;
    const n = r.native;
    const native =
      n && typeof n === 'object'
        ? (() => {
            const x = n as Record<string, unknown>;
            return typeof x.session_id === 'string' &&
              typeof x.agent_name === 'string' &&
              typeof x.project === 'string' &&
              typeof x.through_shell === 'boolean'
              ? {
                  session_id: x.session_id,
                  agent_name: x.agent_name,
                  project: x.project,
                  profile: typeof x.profile === 'string' ? x.profile : null,
                  through_shell: x.through_shell,
                }
              : null;
          })()
        : null;
    out.push({
      terminal: t as unknown as TerminalInfo,
      title: r.title,
      created_at: r.created_at,
      native,
    });
  }
  return out;
}
