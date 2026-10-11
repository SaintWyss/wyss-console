/// <reference types="node" />
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';

// jsdom does not apply a stylesheet, so a view that is mounted and `hidden` but whose class
// sets `display` is hidden for every test and still drawn in the real window: the author's
// `display` beats the browser's `[hidden] { display: none }`. This is what that looked
// like once: Settings, mounted under every other view, was visible below the terminal.
const root = join(__dirname, '..');
const css = readFileSync(join(root, 'src/app.css'), 'utf8');

/** Every .svelte file below a directory. */
function svelteFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory()
      ? svelteFiles(join(directory, entry.name))
      : entry.name.endsWith('.svelte')
        ? [join(directory, entry.name)]
        : [],
  );
}

/** The class of each root <section> that a view hides with the `hidden` attribute. */
const views = svelteFiles(join(root, 'src/lib/features')).flatMap((file) => {
  const found = /<section\s+class="([a-z-]+-feature)"[^>]*\shidden=\{/s.exec(
    readFileSync(file, 'utf8'),
  );
  return found ? [{ file, name: found[1] }] : [];
});

it('finds every view the window mounts and hides', () => {
  expect(views.map((v) => v.name).sort()).toEqual([
    'agent-feature',
    'approvals-feature',
    'events-feature',
    'sessions-feature',
    'settings-feature',
    'terminal-feature',
  ]);
});

/** The rules of the stylesheet, without its comments. */
const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, '');
const rules = [...withoutComments.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(
  (m) => ({
    selectors: m[1].split(',').map((selector) => selector.trim()),
    body: m[2],
  }),
);

it('takes no space for a view that is hidden, whatever display its class sets', () => {
  for (const { name, file } of views) {
    const hiding = rules.filter((rule) =>
      rule.selectors.includes(`.${name}[hidden]`),
    );
    expect(
      hiding.some((rule) => /display:\s*none\s*;/.test(rule.body)),
      `${file}: .${name}[hidden] must be display: none in app.css`,
    ).toBe(true);
  }
});
