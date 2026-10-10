import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import type { TerminalSize } from '../../contracts/terminal';
import { currentAppearance, followAppearance } from './appearance';

export interface TerminalRenderer {
  fit(): TerminalSize | undefined;
  write(data: Uint8Array): Promise<boolean>;
  input(
    text: (data: string) => void,
    binary: (data: string) => void,
  ): () => void;
  setInput(enabled: boolean): void;
  setActive(active: boolean): void;
  /**
   * Tells when a preference changed what fits in the host (a font size), so the
   * terminal can be fitted again. Optional: a renderer without preferences has none.
   */
  onMetricsChange?(listener: () => void): () => void;
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
  const look = currentAppearance();
  const term = new Terminal({
    cols: 80,
    rows: 24,
    scrollback: look.scrollback,
    fontFamily:
      'ui-monospace, "DejaVu Sans Mono", "Liberation Mono", monospace',
    fontSize: look.fontSize,
    cursorStyle: look.cursorStyle,
    cursorBlink: look.cursorBlink,
    disableStdin: true,
    minimumContrastRatio: 4.5,
    linkHandler: { activate: () => undefined },
    theme: palette(),
  });
  const fit = new FitAddon();
  let disposed = false;
  let active = true;
  // Hidden terminals must still answer emulator protocol queries. disableStdin
  // gates those replies too, so visibility gates DOM user input separately.
  term.attachCustomKeyEventHandler(() => active);
  const guardInput = (event: Event) => {
    if (!active) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };
  const guardedEvents = [
    'paste',
    'beforeinput',
    'input',
    'compositionstart',
    'compositionupdate',
    'compositionend',
  ];
  for (const event of guardedEvents)
    host.addEventListener(event, guardInput, true);
  let pending: ((processed: boolean) => void) | undefined;
  const colors = matchMedia('(prefers-color-scheme: dark)');
  const updateTheme = () => {
    term.options.theme = palette();
  };
  colors.addEventListener('change', updateTheme);
  const metrics = new Set<() => void>();
  // Presentation preferences, applied to this terminal whenever they change.
  const unfollow = followAppearance((next) => {
    if (disposed) return;
    const resized = term.options.fontSize !== next.fontSize;
    term.options.fontSize = next.fontSize;
    term.options.scrollback = next.scrollback;
    term.options.cursorStyle = next.cursorStyle;
    term.options.cursorBlink = next.cursorBlink;
    if (resized) for (const listener of [...metrics]) listener();
  });
  try {
    term.loadAddon(fit);
    term.open(host);
  } catch (error) {
    colors.removeEventListener('change', updateTheme);
    unfollow();
    for (const event of guardedEvents)
      host.removeEventListener(event, guardInput, true);
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
    setActive(value) {
      active = value;
      host.inert = !value;
      if (!value && !disposed) term.blur();
    },
    onMetricsChange(listener) {
      metrics.add(listener);
      return () => metrics.delete(listener);
    },
    focus() {
      if (!disposed) term.focus();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      unfollow();
      metrics.clear();
      pending?.(false);
      pending = undefined;
      colors.removeEventListener('change', updateTheme);
      for (const event of guardedEvents)
        host.removeEventListener(event, guardInput, true);
      // Terminal owns and disposes its loaded fit addon.
      term.dispose();
    },
  };
}
