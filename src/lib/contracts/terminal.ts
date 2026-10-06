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
