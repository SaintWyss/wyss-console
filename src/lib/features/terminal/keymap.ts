/**
 * The keys that belong to the Console and not to the program in a terminal (T1, TF3, C-12): one
 * leader chord, Ctrl+Shift, and a letter or an arrow. Everything else reaches the terminal as
 * typed. Ctrl+Shift+C and +V (copy and paste) and +U (an input method's unicode entry) are not
 * taken; the chord is recognised by the physical key, so a dead key or a layout does not
 * change it.
 */
export type Action =
  | { do: 'new-tab' }
  | { do: 'split'; dir: 'row' | 'col' }
  | { do: 'close-pane' }
  | { do: 'search' }
  | { do: 'focus'; way: 'left' | 'right' | 'up' | 'down' | 'next' | 'previous' }
  | { do: 'tab'; step: 1 | -1 }
  | { do: 'jump'; n: number };

export function leaderAction(event: {
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  code: string;
}): Action | undefined {
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey)
    return undefined;
  switch (event.code) {
    case 'KeyT':
      return { do: 'new-tab' };
    case 'KeyD':
      return { do: 'split', dir: 'row' };
    case 'KeyE':
      return { do: 'split', dir: 'col' };
    case 'KeyW':
      return { do: 'close-pane' };
    case 'KeyF':
      return { do: 'search' };
    case 'ArrowLeft':
      return { do: 'focus', way: 'left' };
    case 'ArrowRight':
      return { do: 'focus', way: 'right' };
    case 'ArrowUp':
      return { do: 'focus', way: 'up' };
    case 'ArrowDown':
      return { do: 'focus', way: 'down' };
    case 'Period':
      return { do: 'focus', way: 'next' };
    case 'Comma':
      return { do: 'focus', way: 'previous' };
    case 'PageDown':
      return { do: 'tab', step: 1 };
    case 'PageUp':
      return { do: 'tab', step: -1 };
    default: {
      const digit = /^Digit([1-9])$/.exec(event.code);
      return digit ? { do: 'jump', n: Number(digit[1]) } : undefined;
    }
  }
}

/** What each chord does, for the legend. */
export const KEY_LEGEND: readonly [string, string][] = [
  ['Ctrl+Shift+T', 'New terminal in a new tab'],
  ['Ctrl+Shift+D', 'Split the pane side by side'],
  ['Ctrl+Shift+E', 'Split the pane, one above the other'],
  ['Ctrl+Shift+W', 'Close the pane'],
  ['Ctrl+Shift+F', 'Search the pane’s scrollback'],
  ['Ctrl+Shift+Arrows', 'Move to the pane in that direction'],
  ['Ctrl+Shift+. and ,', 'Next and previous pane'],
  ['Ctrl+Shift+PageDown, PageUp', 'Next and previous tab'],
  ['Ctrl+Shift+1 to 9', 'Go to that tab'],
];
