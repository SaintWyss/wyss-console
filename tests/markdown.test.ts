import { describe, expect, it } from 'vitest';
import {
  classifyUrl,
  renderMarkdown,
  safeUrl,
} from '../src/lib/features/insight/markdown';

/** No element of the output carries an event handler, and no script or frame exists. */
const noActiveMarkup = (html: string) => {
  expect(html).not.toMatch(/<script/i);
  expect(html).not.toMatch(/<(iframe|object|embed|style|svg|math)\b/i);
  expect(html).not.toMatch(/<[a-z][^>]*\son[a-z]+\s*=/i);
};

describe('the README preview renders the Markdown it understands', () => {
  it('headings, paragraphs and rules', () => {
    const html = renderMarkdown(
      '# Title\n\nFirst line\ncontinues here.\n\n---',
    );
    expect(html).toContain('<h1>Title</h1>');
    expect(html).toContain('<p>First line continues here.</p>');
    expect(html).toContain('<hr>');
  });

  it('flat bullet, numbered and task lists', () => {
    const html = renderMarkdown(
      '- one\n- two\n\n1. first\n2. second\n\n- [x] done\n- [ ] todo',
    );
    expect(html).toContain('<ul><li>one</li><li>two</li></ul>');
    expect(html).toContain('<ol><li>first</li><li>second</li></ol>');
    expect(html).toContain('<input type="checkbox" disabled checked> done');
    expect(html).toContain('<input type="checkbox" disabled> todo');
  });

  it('a table with a header and rows, padded to the header width', () => {
    const html = renderMarkdown('| a | b |\n|---|:-:|\n| 1 | 2 |\n| 3 |');
    expect(html).toContain('<th>a</th><th>b</th>');
    expect(html).toContain('<td>1</td><td>2</td>');
    expect(html).toContain('<td>3</td><td></td>');
  });

  it('a fenced code block keeps its text and its language class', () => {
    const html = renderMarkdown('```ts\nconst a = 1 < 2;\n```');
    expect(html).toBe(
      '<pre><code class="language-ts">const a = 1 &lt; 2;</code></pre>',
    );
  });

  it('inline code, bold, italics, strike and links', () => {
    const html = renderMarkdown(
      'Use `npm run check` **now**, *soon* and ~~never~~. See [docs](https://example.com/a?b=1).',
    );
    expect(html).toContain('<code>npm run check</code>');
    expect(html).toContain('<strong>now</strong>');
    expect(html).toContain('<em>soon</em>');
    expect(html).toContain('<del>never</del>');
    expect(html).toContain(
      '<a href="https://example.com/a?b=1" rel="noopener noreferrer nofollow">docs</a>',
    );
  });

  it('a bare URL is linked without formatting the underscores inside it', () => {
    const html = renderMarkdown('Visit https://example.com/a_b_c.');
    expect(html).toContain('<a href="https://example.com/a_b_c"');
    expect(html).toContain('</a>.');
    expect(html).not.toContain('<em>');
  });

  it('blockquotes nest their own Markdown', () => {
    const html = renderMarkdown('> quoted **bold**');
    expect(html).toBe(
      '<blockquote><p>quoted <strong>bold</strong></p></blockquote>',
    );
  });

  it('an https image loads lazily without a referrer', () => {
    const html = renderMarkdown('![logo](https://example.com/logo.png)');
    expect(html).toContain('src="https://example.com/logo.png"');
    expect(html).toContain('alt="logo"');
    expect(html).toContain('referrerpolicy="no-referrer"');
  });
});

describe('untrusted README text cannot run or inject markup', () => {
  it('a script tag is shown as text', () => {
    const html = renderMarkdown('<script>alert(1)</script>');
    expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
    noActiveMarkup(html);
  });

  it('an image with an error handler is shown as text', () => {
    const html = renderMarkdown('<img src=x onerror=alert(1)>');
    expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;');
    noActiveMarkup(html);
  });

  it('a javascript: link becomes an inert anchor', () => {
    const html = renderMarkdown('[x](javascript:alert(1))');
    expect(html).not.toMatch(/javascript:/i);
    expect(html).toContain('<a href="#"');
    noActiveMarkup(html);
  });

  it('obfuscated javascript: links are still blocked', () => {
    for (const url of [
      'JaVaScRiPt:alert(1)',
      'java\tscript:alert(1)',
      '\u0001javascript:alert(1)',
      'data:text/html,<script>alert(1)</script>',
      'vbscript:msgbox(1)',
      '//evil.example/x',
    ]) {
      const html = renderMarkdown(`[x](${url})`);
      expect(html).not.toMatch(/href="(javascript|data|vbscript)/i);
      noActiveMarkup(html);
    }
  });

  it('a quote in a link target cannot break out of the attribute', () => {
    const html = renderMarkdown(
      '[x](https://example.com/"onmouseover=alert(1))',
    );
    expect(html).not.toMatch(/"onmouseover/i);
    noActiveMarkup(html);
  });

  it('a relative path is shown as text with no href', () => {
    const html = renderMarkdown('[config](../../secrets.txt)');
    expect(html).toContain('<span class="md-link-disabled">config</span>');
    expect(html).not.toContain('href=');
  });

  it('an http image does not load and shows its alt text', () => {
    const html = renderMarkdown('![tracker](http://example.com/p.gif)');
    expect(html).toContain('[imagen: tracker]');
    expect(html).not.toContain('<img');
  });

  it('a javascript: image source does not load', () => {
    const html = renderMarkdown('![x](javascript:alert(1))');
    expect(html).not.toContain('<img');
    noActiveMarkup(html);
  });

  it('script inside a heading, table cell, list item and code span stays text', () => {
    const payload = '<script>alert(1)</script>';
    const html = renderMarkdown(
      `# ${payload}\n\n| h |\n|---|\n| ${payload} |\n\n- ${payload}\n\n\`${payload}\``,
    );
    expect(html).not.toContain('<script');
    expect(html.split('&lt;script&gt;').length - 1).toBe(4);
    noActiveMarkup(html);
  });
});

describe('URL policy', () => {
  it('classifies the schemes the preview may open', () => {
    expect(classifyUrl('https://a.example')).toBe('web');
    expect(classifyUrl('http://a.example')).toBe('web');
    expect(classifyUrl('mailto:me@example.com')).toBe('mailto');
    expect(classifyUrl('#section')).toBe('anchor');
    expect(classifyUrl('docs/guide.md')).toBe('relative');
    expect(classifyUrl('ftp://a.example')).toBe('blocked');
  });

  it('safeUrl gives the URL itself when it may open and # otherwise', () => {
    expect(safeUrl('https://a.example')).toBe('https://a.example');
    expect(safeUrl('javascript:alert(1)')).toBe('#');
    expect(safeUrl('docs/guide.md')).toBe('#');
  });
});
