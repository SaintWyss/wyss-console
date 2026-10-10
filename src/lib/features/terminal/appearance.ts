import type { CursorStyle } from '../../contracts/settings';
import { DEFAULT_PREFERENCES } from '../../contracts/settings';

/**
 * How the human terminals look, as the preferences say. Presentation only: it carries no
 * shell, no directory, no command and no environment, and the terminal backend never
 * reads it. One for the window; each renderer follows it for as long as it lives.
 */
export interface TerminalAppearance {
  fontSize: number;
  scrollback: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
}

const FALLBACK: TerminalAppearance = {
  fontSize: DEFAULT_PREFERENCES.terminal.font_size,
  scrollback: DEFAULT_PREFERENCES.terminal.scrollback,
  cursorStyle: DEFAULT_PREFERENCES.terminal.cursor_style,
  cursorBlink: DEFAULT_PREFERENCES.terminal.cursor_blink,
};

let current: TerminalAppearance = FALLBACK;
const followers = new Set<(appearance: TerminalAppearance) => void>();

export function currentAppearance(): TerminalAppearance {
  return current;
}

/** Applies `next` to every terminal that follows. Unchanged values wake nobody. */
export function setAppearance(next: TerminalAppearance) {
  if (
    next.fontSize === current.fontSize &&
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
