import type { TerminalViewState } from './controller';
import { msg } from '../../i18n/msg';

export interface ParserNote {
  text: string;
  values?: Record<string, string | number>;
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
        text: msg(
          'Screen restored · {count} KiB of output was missed while it was kept',
        ),
        values: { count: kib(attach.parser.skipped_bytes) },
        tone: 'warn',
        title: msg(
          'The program printed faster than the screen could be kept, so part of its output never reached it. What is drawn may differ from the program until the program draws again.',
        ),
      };
    }
    if (attach.parser.fidelity === 'tail') {
      return {
        text: msg(
          'Screen rebuilt from the end of the output · {count} KiB before it is not included',
        ),
        values: { count: kib(attach.parser.missing_bytes) },
        tone: 'warn',
        title: msg(
          'The parser that keeps this screen was restarted and only the output the host still held was replayed into it. State set earlier (an alternate screen, a mode) may be missing until the program draws again.',
        ),
      };
    }
    const omitted = attach.history.omitted + attach.history.evicted;
    if (omitted > 0) {
      return {
        text: msg('Screen restored · {count} older lines not shown'),
        values: { count: omitted },
        tone: 'idle',
        title: msg(
          'The screen and its recent history were redrawn from the host. Older history was past what is kept.',
        ),
      };
    }
  }
  if (attach?.mode === 'replay') {
    return {
      text: msg('Output replayed · no screen state was kept'),
      tone: 'warn',
      title: msg(
        'The host had no screen for this terminal ({reason}), so what it kept of the output was replayed and the program was asked to draw again.',
      ),
      values: { reason: attach.reason.replaceAll('_', ' ') },
    };
  }
  if (view.parser?.fidelity === 'gap' && view.parser.health === 'healthy') {
    return {
      text: msg('Output was missed while the screen was kept'),
      tone: 'warn',
      title: msg(
        'The program printed faster than the screen could be kept. The terminal itself is complete; a window opened later may show a screen that differs until the program draws again.',
      ),
    };
  }
  const health = view.parser?.health;
  if (health === 'restarting') {
    return {
      text: msg('Screen parser restarting'),
      tone: 'warn',
      title: msg(
        'The terminal itself is not affected. The screen state is being rebuilt from the output the host still holds.',
      ),
    };
  }
  if (health === 'unavailable' && view.state === 'running') {
    return {
      text: msg('Screen parser unavailable'),
      tone: 'warn',
      title: msg(
        'The terminal works. If this window is closed and opened again, the output the host kept is replayed instead of a saved screen.',
      ),
    };
  }
  return null;
}
