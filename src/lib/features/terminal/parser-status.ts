import type { TerminalViewState } from './controller';

export interface ParserNote {
  text: string;
  /** `idle`: information; `warn`: something is missing or degraded. */
  tone: 'idle' | 'warn';
  title: string;
}

const kib = (bytes: number) => Math.max(1, Math.round(bytes / 1024));

/**
 * What a terminal says about the screen state the host keeps for it, or nothing when all is as
 * usual. Plain facts: how the screen came to be drawn, what is missing from it, whether the parser
 * that keeps it is down. Never "restored" without saying what was not.
 */
export function parserNote(view: TerminalViewState): ParserNote | null {
  const attach = view.attach;
  if (attach?.mode === 'screen') {
    if (attach.parser.fidelity === 'gap') {
      return {
        text: `Screen restored · ${kib(attach.parser.skipped_bytes)} KiB of output was missed while it was kept`,
        tone: 'warn',
        title:
          'The program printed faster than the screen could be kept, so part of its output never reached it. What is drawn may differ from the program until the program draws again.',
      };
    }
    if (attach.parser.fidelity === 'tail') {
      return {
        text: `Screen rebuilt from the end of the output · ${kib(attach.parser.missing_bytes)} KiB before it is not included`,
        tone: 'warn',
        title:
          'The parser that keeps this screen was restarted and only the output the host still held was replayed into it. State set earlier (an alternate screen, a mode) may be missing until the program draws again.',
      };
    }
    const omitted = attach.history.omitted + attach.history.evicted;
    if (omitted > 0) {
      return {
        text: `Screen restored · ${omitted} older lines not shown`,
        tone: 'idle',
        title:
          'The screen and its recent history were redrawn from the host. Older history was past what is kept.',
      };
    }
  }
  if (attach?.mode === 'replay') {
    return {
      text: 'Output replayed · no screen state was kept',
      tone: 'warn',
      title: `The host had no screen for this terminal (${attach.reason.replaceAll('_', ' ')}), so what it kept of the output was replayed and the program was asked to draw again.`,
    };
  }
  if (view.parser?.fidelity === 'gap' && view.parser.health === 'healthy') {
    return {
      text: 'Output was missed while the screen was kept',
      tone: 'warn',
      title:
        'The program printed faster than the screen could be kept. The terminal itself is complete; a window opened later may show a screen that differs until the program draws again.',
    };
  }
  const health = view.parser?.health;
  if (health === 'restarting') {
    return {
      text: 'Screen parser restarting',
      tone: 'warn',
      title:
        'The terminal itself is not affected. The screen state is being rebuilt from the output the host still holds.',
    };
  }
  if (health === 'unavailable' && view.state === 'running') {
    return {
      text: 'Screen parser unavailable',
      tone: 'warn',
      title:
        'The terminal works. If this window is closed and opened again, the output the host kept is replayed instead of a saved screen.',
    };
  }
  return null;
}
