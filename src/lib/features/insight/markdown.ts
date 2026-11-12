/**
 * A small Markdown renderer for the README preview. No dependencies and no raw HTML: every
 * piece of source text is escaped before it becomes markup, so a README cannot run script,
 * draw a foreign element or hand the webview a local file.
 *
 * Links: `https:`, `http:`, `mailto:` and `#anchor` open; a relative path is shown as text
 * (there is no file access from the webview); every other scheme becomes `#`.
 * Images: only `https:` loads; anything else shows as a short note with its alt text.
 */

const ENTITY: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
};
const UNENTITY: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  '#39': "'",
};

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ENTITY[c]);
}

/** Reverses `escapeHtml` only; nothing else is decoded. */
function unescapeHtml(text: string): string {
  return text.replace(
    /&(amp|lt|gt|quot|#39);/g,
    (_, name: string) => UNENTITY[name],
  );
}

export type UrlKind = 'web' | 'mailto' | 'anchor' | 'relative' | 'blocked';

/** C0 controls and DEL: browsers strip some of these from URLs, so they never reach an href. */
function hasControlCharacter(text: string): boolean {
  for (let k = 0; k < text.length; k++) {
    const code = text.charCodeAt(k);
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

/** Classifies a raw (unescaped) URL. Control characters and any other scheme are blocked. */
export function classifyUrl(raw: string): UrlKind {
  const url = raw.trim();
  if (url === '' || hasControlCharacter(url)) return 'blocked';
  if (/^https?:\/\/\S/i.test(url)) return 'web';
  if (/^mailto:\S/i.test(url)) return 'mailto';
  if (url.startsWith('#')) return 'anchor';
  if (/^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith('//'))
    return 'blocked';
  return 'relative';
}

/** The href to use for a raw URL: the URL itself when it may open, `#` otherwise. */
export function safeUrl(raw: string): string {
  const kind = classifyUrl(raw);
  return kind === 'web' || kind === 'mailto' || kind === 'anchor'
    ? raw.trim()
    : '#';
}

// ---- inline (runs on text that is already escaped) ------------------------------------------

const INLINE_SOURCE = [
  '`([^`\\n]+)`', // 1 code span
  '!\\[([^\\]\\n]*)\\]\\(([^)\\s]+)\\)', // 2 alt, 3 image source
  '\\[([^\\]\\n]+)\\]\\(([^)\\s]+)\\)', // 4 link text, 5 link target
  '(https?:\\/\\/\\S+)', // 6 bare URL
  '\\*\\*([^*\\n]+?)\\*\\*', // 7 bold
  '~~([^~\\n]+?)~~', // 8 strike
  '\\*([^*\\n]+?)\\*', // 9 emphasis
  '(?<!\\w)_([^_\\n]+?)_(?!\\w)', // 10 emphasis with underscores
].join('|');

function inlineEscaped(text: string): string {
  return text.replace(
    new RegExp(INLINE_SOURCE, 'g'),
    (match: string, ...g: Array<string | undefined>) => {
      const [
        code,
        imgAlt,
        imgSrc,
        linkText,
        linkUrl,
        bare,
        bold,
        strike,
        em,
        emU,
      ] = g;
      if (code !== undefined) return `<code>${code}</code>`;
      if (imgAlt !== undefined) return image(imgAlt, imgSrc ?? '');
      if (linkText !== undefined) return link(linkText, linkUrl ?? '');
      if (bare !== undefined) {
        // Trailing sentence punctuation is text, not part of the link.
        const tail = /[.,;:!?)]+$/.exec(bare)?.[0] ?? '';
        const core = tail ? bare.slice(0, -tail.length) : bare;
        return anchor(core, core) + tail;
      }
      if (bold !== undefined) return `<strong>${inlineEscaped(bold)}</strong>`;
      if (strike !== undefined) return `<del>${inlineEscaped(strike)}</del>`;
      if (em !== undefined) return `<em>${inlineEscaped(em)}</em>`;
      if (emU !== undefined) return `<em>${inlineEscaped(emU)}</em>`;
      return match;
    },
  );
}

/** A link whose text is already markup; the href is checked on the unescaped URL. */
function anchor(htmlText: string, escapedUrl: string): string {
  const href = escapeHtml(safeUrl(unescapeHtml(escapedUrl)));
  return `<a href="${href}" rel="noopener noreferrer nofollow">${htmlText}</a>`;
}

function link(escapedText: string, escapedUrl: string): string {
  if (classifyUrl(unescapeHtml(escapedUrl)) === 'relative') {
    return `<span class="md-link-disabled">${inlineEscaped(escapedText)}</span>`;
  }
  return anchor(inlineEscaped(escapedText), escapedUrl);
}

function image(escapedAlt: string, escapedSrc: string): string {
  const raw = unescapeHtml(escapedSrc).trim();
  if (/^https:\/\/\S/i.test(raw)) {
    return `<img src="${escapeHtml(raw)}" alt="${escapedAlt}" loading="lazy" referrerpolicy="no-referrer">`;
  }
  return `<span class="md-image-note">[imagen: ${escapedAlt}]</span>`;
}

/** Inline Markdown for raw source text: escapes first, then formats. */
function inline(raw: string): string {
  return inlineEscaped(escapeHtml(raw));
}

// ---- blocks ----------------------------------------------------------------------------------

const FENCE = /^\s{0,3}(`{3,}|~{3,})\s*([\w+#.-]*)/;
const HEADING = /^\s{0,3}(#{1,6})(?:\s+(.*?))?\s*$/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;
const LIST_ITEM = /^\s*([-*+]|\d+[.)])\s+(.*)$/;
const TASK = /^\[([ xX])\](?:\s+(.*))?$/;
const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

function isClosingFence(line: string, opening: string): boolean {
  const m = /^\s{0,3}(`{3,}|~{3,})\s*$/.exec(line);
  return m !== null && m[1][0] === opening[0] && m[1].length >= opening.length;
}

function isTableStart(lines: string[], i: number): boolean {
  return (
    i + 1 < lines.length &&
    lines[i].includes('|') &&
    lines[i + 1].includes('|') &&
    TABLE_RULE.test(lines[i + 1])
  );
}

function startsBlock(lines: string[], i: number): boolean {
  const line = lines[i];
  return (
    FENCE.test(line) ||
    HEADING.test(line) ||
    RULE.test(line) ||
    QUOTE.test(line) ||
    LIST_ITEM.test(line) ||
    isTableStart(lines, i)
  );
}

function splitRow(line: string): string[] {
  let s = line.trim();
  if (s.startsWith('|')) s = s.slice(1);
  if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
  return s.split('|').map((cell) => cell.trim());
}

function listItem(text: string): string {
  const task = TASK.exec(text);
  if (task) {
    const checked = task[1] !== ' ' ? ' checked' : '';
    return `<li class="md-task"><input type="checkbox" disabled${checked}> ${inline(task[2] ?? '')}</li>`;
  }
  return `<li>${inline(text)}</li>`;
}

/**
 * Renders a README. Lists are flat (nesting is shown at the top level); nothing else in the
 * source reaches the output unescaped.
 */
export function renderMarkdown(src: string): string {
  const lines = src.replace(/\r\n?/g, '\n').split('\n');
  const out: string[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trim() === '') {
      i++;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const opening = fence[1];
      const body: string[] = [];
      i++;
      while (i < lines.length && !isClosingFence(lines[i], opening)) {
        body.push(lines[i]);
        i++;
      }
      i++; // the closing fence, or the end of the source
      const lang = fence[2] ? ` class="language-${escapeHtml(fence[2])}"` : '';
      out.push(`<pre><code${lang}>${escapeHtml(body.join('\n'))}</code></pre>`);
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      const level = heading[1].length;
      const text = (heading[2] ?? '').replace(/\s+#+$/, '');
      out.push(`<h${level}>${inline(text)}</h${level}>`);
      i++;
      continue;
    }

    if (RULE.test(line)) {
      out.push('<hr>');
      i++;
      continue;
    }

    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) {
        body.push(QUOTE.exec(lines[i])?.[1] ?? '');
        i++;
      }
      out.push(`<blockquote>${renderMarkdown(body.join('\n'))}</blockquote>`);
      continue;
    }

    if (isTableStart(lines, i)) {
      const head = splitRow(line);
      i += 2;
      const rows: string[][] = [];
      while (
        i < lines.length &&
        lines[i].trim() !== '' &&
        lines[i].includes('|')
      ) {
        rows.push(splitRow(lines[i]));
        i++;
      }
      const headCells = head.map((c) => `<th>${inline(c)}</th>`).join('');
      const bodyRows = rows
        .map((row) => {
          const cells = head.map((_, c) => `<td>${inline(row[c] ?? '')}</td>`);
          return `<tr>${cells.join('')}</tr>`;
        })
        .join('');
      out.push(
        `<div class="md-table-wrap"><table><thead><tr>${headCells}</tr></thead><tbody>${bodyRows}</tbody></table></div>`,
      );
      continue;
    }

    const item = LIST_ITEM.exec(line);
    if (item) {
      const ordered = /\d/.test(item[1]);
      const tag = ordered ? 'ol' : 'ul';
      const items: string[] = [];
      while (i < lines.length) {
        const next = LIST_ITEM.exec(lines[i]);
        if (!next || RULE.test(lines[i]) || /\d/.test(next[1]) !== ordered)
          break;
        items.push(listItem(next[2]));
        i++;
      }
      out.push(`<${tag}>${items.join('')}</${tag}>`);
      continue;
    }

    // A paragraph: consecutive lines until a blank line or another block.
    const para: string[] = [line.trim()];
    i++;
    while (
      i < lines.length &&
      lines[i].trim() !== '' &&
      !startsBlock(lines, i)
    ) {
      para.push(lines[i].trim());
      i++;
    }
    out.push(`<p>${inline(para.join(' '))}</p>`);
  }
  return out.join('\n');
}
