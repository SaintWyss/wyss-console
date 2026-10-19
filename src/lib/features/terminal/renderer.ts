import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import type { TerminalSize } from '../../contracts/terminal';
import { currentAppearance, followAppearance } from './appearance';
import { leaderAction } from './keymap';

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
  /** Finds `query` in the scrollback and the screen and shows the match `step` from the last. */
  search?(
    query: string,
    options: { caseSensitive: boolean; step: 1 | -1 | 0 },
  ): SearchResult;
  clearSearch?(): void;
  dispose(): void;
}

export interface SearchResult {
  /** How many matches there are (at most `MAX_MATCHES`). */
  total: number;
  /** The one shown, from 1; 0 when there is none. */
  current: number;
}
/** More matches than this are not counted: a search stays cheap. */
export const MAX_MATCHES = 5000;

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
  // The Console's own chords (Ctrl+Shift+…) are not the program's: they go on to the window.
  term.attachCustomKeyEventHandler((event) => active && !leaderAction(event));
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
  let searchAt = -1;
  let searchQuery = '';
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
    search(query, options) {
      if (disposed || query === '') return { total: 0, current: 0 };
      const buffer = term.buffer.active;
      const needle = options.caseSensitive ? query : query.toLowerCase();
      const found: { row: number; col: number }[] = [];
      for (
        let row = 0;
        row < buffer.length && found.length < MAX_MATCHES;
        row++
      ) {
        const line = buffer.getLine(row);
        if (!line) continue;
        const text = line.translateToString(false);
        const hay = options.caseSensitive ? text : text.toLowerCase();
        for (
          let at = hay.indexOf(needle);
          at >= 0 && found.length < MAX_MATCHES;
          at = hay.indexOf(needle, at + Math.max(1, needle.length))
        )
          found.push({ row, col: at });
      }
      if (found.length === 0) {
        term.clearSelection();
        searchAt = -1;
        return { total: 0, current: 0 };
      }
      // Forward from the match shown, or from the top of what is on screen for the first.
      const first = buffer.viewportY;
      const start =
        searchAt >= 0 && searchQuery === `${options.caseSensitive}${query}`
          ? searchAt + options.step
          : Math.max(
              0,
              found.findIndex((m) => m.row >= first),
            );
      const index = ((start % found.length) + found.length) % found.length;
      searchAt = index;
      searchQuery = `${options.caseSensitive}${query}`;
      const match = found[index]!;
      term.select(match.col, match.row, query.length);
      term.scrollToLine(Math.max(0, match.row - Math.floor(term.rows / 2)));
      return { total: found.length, current: index + 1 };
    },
    clearSearch() {
      searchAt = -1;
      searchQuery = '';
      if (!disposed) term.clearSelection();
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
