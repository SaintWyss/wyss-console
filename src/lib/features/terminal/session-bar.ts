import type { SessionGit } from '../../contracts/terminal';
import type { Profile } from '../../contracts/native';
import { msg, type Phrase } from '../../i18n/msg';
import type { NativeTag } from './session-manager';

/** One part of the session bar: words, and the state tone they carry (never color alone). */
export interface BarPart {
  key: 'folder' | 'branch' | 'changes' | 'ahead' | 'nogit';
  phrase: Phrase;
  /** Untranslated text (a path, a branch name). */
  raw?: string;
  tone?: 'ok' | 'warn';
}

/** `/home/me/x` → `~/x` for reading; the full path stays in the tooltip. */
export function homeShort(path: string, home: string | null): string {
  if (home && (path === home || path.startsWith(`${home}/`)))
    return `~${path.slice(home.length)}`;
  return path;
}

/** The left side of the bar: folder, then what git says about it. `unknown` shows the folder only. */
export function gitParts(
  git: SessionGit | null,
  fallbackDir: string,
): BarPart[] {
  const dir = git?.dir ?? fallbackDir;
  const home = git?.home ?? null;
  const parts: BarPart[] = [];
  if (dir)
    parts.push({
      key: 'folder',
      phrase: { text: '{folder}', values: { folder: homeShort(dir, home) } },
      raw: dir,
    });
  if (!git || git.kind === 'unknown') return parts;
  if (git.kind === 'no_repo') {
    parts.push({ key: 'nogit', phrase: { text: msg('no git') } });
    return parts;
  }
  parts.push({
    key: 'branch',
    phrase: git.branch
      ? { text: '{branch}', values: { branch: git.branch } }
      : { text: msg('detached HEAD') },
  });
  const more = git.truncated ? '+' : '';
  parts.push(
    git.changes === 0
      ? { key: 'changes', phrase: { text: msg('✓ all committed') }, tone: 'ok' }
      : {
          key: 'changes',
          phrase:
            git.changes === 1 && !more
              ? { text: msg('● 1 change not committed') }
              : {
                  text: msg('● {count} changes not committed'),
                  values: { count: `${git.changes}${more}` },
                },
          tone: 'warn',
        },
  );
  if (git.ahead !== null && git.ahead > 0)
    parts.push({
      key: 'ahead',
      phrase: {
        text: msg('↑{count} not pushed'),
        values: { count: git.ahead },
      },
      tone: 'warn',
    });
  return parts;
}

/** What runs in the session, as the right side of the bar says it. */
export function agentPart(
  native: NativeTag | undefined,
  profiles: readonly Profile[],
): { kind: Phrase; detail: string | null } {
  if (!native) return { kind: { text: msg('Terminal') }, detail: null };
  const profile = native.profile
    ? profiles.find((p) => p.body.name === native.profile)
    : undefined;
  const model = profile?.body.model ?? null;
  return {
    kind: { text: msg('Native · {agent}'), values: { agent: native.agent } },
    detail: model ?? native.profile,
  };
}
