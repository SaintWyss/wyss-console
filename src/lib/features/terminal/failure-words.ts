import { msg } from '../../i18n/msg';

/**
 * What a terminal's closed failure words mean, for a person. The word itself stays available
 * (as a title) for a bug report; it is never the only thing shown.
 */
const FAILURES: Readonly<Record<string, string>> = {
  invalid_dimensions: msg('The terminal size was refused.'),
  invalid_cwd: msg(
    'The folder to start in does not exist or cannot be opened.',
  ),
  spawn_failed: msg('The shell could not be started.'),
  write_failed: msg('Input could not be written to the terminal.'),
  resize_failed: msg('The terminal could not be resized.'),
  busy: msg('The terminal stayed busy and did not take the input.'),
  limit_reached: msg(
    'The limit of open terminals was reached. Close one first.',
  ),
  operation_timeout: msg('The terminal did not answer in time.'),
  close_timeout: msg('The terminal did not close in time.'),
  close_failed: msg('The terminal could not be closed.'),
  renderer_error: msg('The terminal could not be drawn in this window.'),
  host_unavailable: msg(
    'The terminal host is not answering. Nothing was sent.',
  ),
  host_outcome_unknown: msg(
    'The terminal host did not confirm this request. It may or may not have taken effect, and it was not sent again. Check the kept terminals before starting another.',
  ),
  terminal_error: msg('The terminal stopped with an error.'),
};

export function failureWords(code: string | undefined): string {
  return FAILURES[code ?? 'terminal_error'] ?? FAILURES.terminal_error;
}

/** A notice about input on a terminal that keeps running. */
export type InputNotice = 'input_outcome_unknown' | 'input_limit';

const NOTICES: Readonly<Record<InputNotice, string>> = {
  input_outcome_unknown: msg(
    'The host did not confirm the last input: it may or may not have reached the program. It was not sent again, and input still waiting was dropped. Check the screen before typing on.',
  ),
  input_limit: msg(
    'That input was not sent: with what is already waiting it is more than a terminal holds (64 KiB). Nothing of it reached the program; what was waiting before it still goes.',
  ),
};

export function inputNoticeWords(notice: InputNotice): string {
  return NOTICES[notice];
}
