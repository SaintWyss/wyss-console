import {
  closeTerminal,
  createTerminal,
  readTerminalOutput,
  resizeTerminal,
  writeTerminal,
} from '../../desktop/terminal-client';
import type { TerminalInfo } from '../../contracts/terminal';
import type { TerminalId } from '../../contracts/identities';

const PREVIEW_LIMIT = 8192;
export interface SmokeResult {
  info: TerminalInfo;
  preview: string;
  bytes: number;
}

/** Removable 1A diagnostic only. No emulator, scrollback, ANSI parsing or event log. */
export async function runPtySmoke(
  progress: (preview: string) => void,
): Promise<SmokeResult> {
  let info: TerminalInfo | undefined;
  let closed = false;
  let preview = '';
  let bytes = 0;
  const decoder = new TextDecoder();
  async function readUntil(id: TerminalId, expected: string[]) {
    const deadline = Date.now() + 8000;
    let phase = '';
    while (Date.now() < deadline) {
      const output = await readTerminalOutput(id);
      bytes += output.data.length;
      const text = decoder.decode(output.data, { stream: true });
      phase = (phase + text).slice(-PREVIEW_LIMIT);
      preview = (preview + text).slice(-PREVIEW_LIMIT);
      progress(preview);
      if (output.truncated) throw new Error('diagnostic output truncated');
      // Diagnostic comparison only; byte transport and preview remain unchanged.
      const observed = phase.replaceAll('\r', '');
      if (expected.every((marker) => observed.includes(marker))) return;
      if (output.eof)
        throw new Error('shell exited before diagnostic completed');
      // One outstanding pull, no frame/chunk task queue or accumulating history.
      await new Promise<void>((resolve) => setTimeout(resolve, 25));
    }
    throw new Error('diagnostic timeout');
  }
  try {
    info = await createTerminal({ rows: 24, cols: 80 });
    await writeTerminal(
      info.terminal_id,
      new TextEncoder().encode("printf 'WYSS_PTY_OK\\n'; pwd\n"),
    );
    await readUntil(info.terminal_id, ['WYSS_PTY_OK\n', `${info.cwd}\n`]);
    info = await resizeTerminal(info.terminal_id, { rows: 32, cols: 100 });
    await writeTerminal(
      info.terminal_id,
      new TextEncoder().encode("stty size; printf 'WYSS_RESIZE_DONE\\n'\n"),
    );
    await readUntil(info.terminal_id, ['32 100\n', 'WYSS_RESIZE_DONE\n']);
    info = await closeTerminal(info.terminal_id);
    closed = true;
    if (info.state !== 'closed') throw new Error('terminal did not close');
    return { info, preview, bytes };
  } finally {
    if (info && !closed)
      await closeTerminal(info.terminal_id).catch(() => undefined);
  }
}
