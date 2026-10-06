import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import type { TerminalSize } from '../../contracts/terminal';

export interface TerminalRenderer {
  fit(): TerminalSize | undefined;
  write(data: Uint8Array): Promise<boolean>;
  input(
    text: (data: string) => void,
    binary: (data: string) => void,
  ): () => void;
  setInput(enabled: boolean): void;
  focus(): void;
  dispose(): void;
}

export function createRenderer(host: HTMLElement): TerminalRenderer {
  const palette = () => {
    const css = getComputedStyle(document.documentElement);
    return {
      background: css.getPropertyValue('--terminal-bg').trim(),
      foreground: css.getPropertyValue('--terminal-fg').trim(),
      cursor: css.getPropertyValue('--terminal-fg').trim(),
    };
  };
  const term = new Terminal({
    cols: 80,
    rows: 24,
    scrollback: 5000,
    fontFamily:
      'ui-monospace, "DejaVu Sans Mono", "Liberation Mono", monospace',
    fontSize: 16,
    cursorBlink: false,
    disableStdin: true,
    minimumContrastRatio: 4.5,
    linkHandler: { activate: () => undefined },
    theme: palette(),
  });
  const fit = new FitAddon();
  let disposed = false;
  let pending: ((processed: boolean) => void) | undefined;
  const colors = matchMedia('(prefers-color-scheme: dark)');
  const updateTheme = () => {
    term.options.theme = palette();
  };
  colors.addEventListener('change', updateTheme);
  try {
    term.loadAddon(fit);
    term.open(host);
  } catch (error) {
    colors.removeEventListener('change', updateTheme);
    term.dispose();
    throw error;
  }
  return {
    fit() {
      if (disposed || host.clientWidth <= 0 || host.clientHeight <= 0) return;
      const size = fit.proposeDimensions();
      if (!size || !Number.isFinite(size.cols) || !Number.isFinite(size.rows))
        return;
      const cols = Math.max(2, Math.min(512, size.cols));
      const rows = Math.max(1, Math.min(512, size.rows));
      if (term.cols !== cols || term.rows !== rows) term.resize(cols, rows);
      return { rows, cols };
    },
    write(data) {
      if (disposed) return Promise.resolve(false);
      if (pending) return Promise.reject(new Error('renderer_busy'));
      return new Promise<boolean>((resolve) => {
        pending = resolve;
        term.write(data, () => {
          pending = undefined;
          resolve(!disposed);
        });
      });
    },
    input(text, binary) {
      const textListener = term.onData(text);
      const binaryListener = term.onBinary(binary);
      return () => {
        textListener.dispose();
        binaryListener.dispose();
      };
    },
    setInput(enabled) {
      term.options.disableStdin = !enabled;
    },
    focus() {
      if (!disposed) term.focus();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      pending?.(false);
      pending = undefined;
      colors.removeEventListener('change', updateTheme);
      // Terminal owns and disposes its loaded fit addon.
      term.dispose();
    },
  };
}
