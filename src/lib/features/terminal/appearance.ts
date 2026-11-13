import type { AppearanceTheme, CursorStyle } from '../../contracts/settings';
import { DEFAULT_PREFERENCES } from '../../contracts/settings';

/**
 * How the human terminals look, as the preferences say. Presentation only: it carries no
 * shell, no directory, no command and no environment, and the terminal backend never
 * reads it. One for the window; each renderer follows it for as long as it lives.
 */
export interface TerminalAppearance {
  theme: AppearanceTheme;
  fontSize: number;
  scrollback: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
}

const FALLBACK: TerminalAppearance = {
  theme: DEFAULT_PREFERENCES.interface.theme,
  fontSize: DEFAULT_PREFERENCES.terminal.font_size,
  scrollback: DEFAULT_PREFERENCES.terminal.scrollback,
  cursorStyle: DEFAULT_PREFERENCES.terminal.cursor_style,
  cursorBlink: DEFAULT_PREFERENCES.terminal.cursor_blink,
};

export interface TerminalPalette {
  background: string;
  foreground: string;
  cursor: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

const TERMINAL_PALETTES: Record<AppearanceTheme, TerminalPalette> = {
  dark: {
    background: '#0e0e0e',
    foreground: '#ececec',
    cursor: '#ececec',
    black: '#8e8e8e',
    red: '#ff8272',
    green: '#b4fa72',
    yellow: '#fefdc2',
    blue: '#a5d5fe',
    magenta: '#ff8ffd',
    cyan: '#d0d1fe',
    white: '#f1f1f1',
    brightBlack: '#a8a8a8',
    brightRed: '#ffc4bd',
    brightGreen: '#d6fcb9',
    brightYellow: '#fefdd5',
    brightBlue: '#c1e3fe',
    brightMagenta: '#ffb1fe',
    brightCyan: '#e5e6fe',
    brightWhite: '#feffff',
  },
  light: {
    background: '#ffffff',
    foreground: '#111111',
    cursor: '#111111',
    black: '#111827',
    red: '#a40000',
    green: '#075a2c',
    yellow: '#644200',
    blue: '#004f9e',
    magenta: '#6b2292',
    cyan: '#00636b',
    white: '#475569',
    brightBlack: '#343f50',
    brightRed: '#7f0000',
    brightGreen: '#004820',
    brightYellow: '#503400',
    brightBlue: '#003d7a',
    brightMagenta: '#571578',
    brightCyan: '#004b52',
    brightWhite: '#1f2937',
  },
  high_contrast: {
    background: '#000000',
    foreground: '#ffffff',
    cursor: '#ffffff',
    black: '#c5c5c5',
    red: '#ff5c5c',
    green: '#73ff73',
    yellow: '#fff45c',
    blue: '#78b5ff',
    magenta: '#f0aaff',
    cyan: '#55ffff',
    white: '#ffffff',
    brightBlack: '#f0f0f0',
    brightRed: '#ff8585',
    brightGreen: '#a3ffa3',
    brightYellow: '#fffaa8',
    brightBlue: '#aad1ff',
    brightMagenta: '#ffccff',
    brightCyan: '#a6ffff',
    brightWhite: '#ffffff',
  },
};

/** Accessible ANSI palette, with foreground colors chosen for this theme's terminal surface. */
export function terminalPalette(theme: AppearanceTheme): TerminalPalette {
  return { ...TERMINAL_PALETTES[theme] };
}

let current: TerminalAppearance = FALLBACK;
const followers = new Set<(appearance: TerminalAppearance) => void>();

export function currentAppearance(): TerminalAppearance {
  return current;
}

/** Applies `next` to every terminal that follows. Unchanged values wake nobody. */
export function setAppearance(next: TerminalAppearance) {
  if (
    next.fontSize === current.fontSize &&
    next.theme === current.theme &&
    next.scrollback === current.scrollback &&
    next.cursorStyle === current.cursorStyle &&
    next.cursorBlink === current.cursorBlink
  )
    return;
  current = next;
  for (const follower of [...followers]) follower(current);
}

/** Calls `follower` with every later change. Returns how to stop. */
export function followAppearance(
  follower: (appearance: TerminalAppearance) => void,
): () => void {
  followers.add(follower);
  return () => followers.delete(follower);
}

/** Back to the defaults. For tests, which share this module. */
export function resetAppearance() {
  current = FALLBACK;
  followers.clear();
}
