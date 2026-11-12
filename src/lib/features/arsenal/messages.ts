import type { Imported } from '../../contracts/arsenal';
import { ArsenalCallError } from '../../desktop/arsenal-client';
import { msg } from '../../i18n/msg';

/** What the owner reads for each failure code. Errors say what happened and what to do. */
const CODE_TEXT: Record<string, string> = {
  storage_unavailable: msg(
    'The Arsenal library could not be opened. Check that the Wyss data folder is writable, then Refresh.',
  ),
  storage_corrupt: msg(
    'The Arsenal library is damaged, so nothing was changed. Keep the file and report it.',
  ),
  storage_busy: msg('Another change is in progress. Try again in a moment.'),
  storage_unsupported: msg(
    'This library was written by a newer version of Wyss Console.',
  ),
  discovery_unavailable: msg(
    'Your home folder is not known, so skills cannot be read. Refresh to try again.',
  ),
  not_found: msg(
    'That item is no longer in the library or in your skill folders. Refresh and try again.',
  ),
  invalid_request: msg(
    'The request was not valid. Check the fields and try again.',
  ),
  not_importable: msg('This kind is listed only. Arsenal does not copy it.'),
  too_large: msg('This file is too large to show here.'),
  binary_file: msg('This file is not text, so it cannot be shown here.'),
  internal: msg(
    'Arsenal returned something unexpected. Refresh and try again.',
  ),
};

/** Why a skill cannot be imported. Each is a fact found by the scan, never a guess. */
const BLOCK_TEXT: Record<string, string> = {
  needs_secret: msg(
    'A file holds something that looks like a secret. Remove it from the skill to import it.',
  ),
  no_skill_file: msg('This folder has no SKILL.md, so it is not a skill.'),
  symbolic_link: msg(
    'This skill is a symbolic link. Arsenal does not follow links out of a skill folder.',
  ),
  too_many_files: msg('This skill has more files than the import limit.'),
  tree_too_large: msg('This skill is larger than the import limit.'),
  file_too_large: msg(
    'One file in this skill is larger than the import limit.',
  ),
  too_deep: msg('This skill nests folders deeper than the import limit.'),
  unreadable: msg('A file of this skill could not be read.'),
  unstable: msg('This skill changed while it was being read. Scan again.'),
  provider_managed: msg(
    'Codex installs and replaces this skill itself. Arsenal lists it and does not copy it.',
  ),
  shared_path: msg(
    'This project is in a folder another account can write. Arsenal lists it as read only and does not copy it.',
  ),
};

const SKILL_NOT_IMPORTABLE = msg('This skill cannot be imported.');

export function failureMessage(error: unknown): string {
  if (!(error instanceof ArsenalCallError)) return CODE_TEXT.internal;
  if (error.code === 'blocked') {
    return BLOCK_TEXT[error.reason ?? ''] ?? SKILL_NOT_IMPORTABLE;
  }
  return CODE_TEXT[error.code] ?? CODE_TEXT.internal;
}

export function blockedText(reason: string): string {
  return BLOCK_TEXT[reason] ?? SKILL_NOT_IMPORTABLE;
}

export function importMessage(name: string, imported: Imported): string {
  switch (imported.outcome) {
    case 'created':
      return `Imported ${name} as revision ${imported.revision}.`;
    case 'new_revision':
      return `Imported ${name} as revision ${imported.revision}. The source had changed.`;
    case 'unchanged':
      return `${name} is already imported at revision ${imported.revision}. Nothing changed.`;
  }
}
