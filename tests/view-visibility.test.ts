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
    'configuration-feature',
    'distributors-feature',
    'events-feature',
    'native-feature',
    'native-feature',
    'native-feature',
    'pipelines-feature',
    'projects-feature',
    'providers-feature',
    'runtimes-feature',
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

it('never removes a status: the status bar is hidden only on the Console, which has its session bar', () => {
  // A window of 800 px or less once dropped the Codex status, and one of 560 px or less
  // the authority's: which of the three is broken could not be seen. None may be taken away.
  const statuses = [
    '.status-cluster',
    '.pill',
    '.pill.terminal',
    '.pill.agent',
    '.pill.authority',
    '.dot',
  ];
  const removed = rules
    .filter((rule) => /display:\s*none/.test(rule.body))
    .flatMap((rule) => rule.selectors)
    .filter((selector) => statuses.includes(selector));
  expect(removed).toEqual([]);
  const app = readFileSync(join(root, 'src/App.svelte'), 'utf8');
  expect(app).toMatch(/<StatusBar\s+hidden=\{activeView === 'console'\}/);
});

it('keeps the rail at 52 px: it never widens over the view', () => {
  expect(css).toMatch(/--sidebar-w:\s*52px;/);
  const widening = rules.filter(
    (rule) =>
      rule.selectors.some((selector) => selector.startsWith('.sidebar')) &&
      /width:\s*232px/.test(rule.body),
  );
  expect(widening).toEqual([]);
});

it('puts the Settings sections in two columns only in a wide window', () => {
  // jsdom lays nothing out: what can be held here is that the rule exists, that it is
  // conditional on the width of the view, and that what is not a section (a notice, the
  // lede) is not squeezed into one of the columns.
  const wide =
    /@container \(min-width: 1000px\) \{\s*\.settings-body \{([^}]*)\}\s*\.settings-body > :not\(\.settings-section\) \{([^}]*)\}/.exec(
      withoutComments,
    );
  expect(wide, 'the two-column rule of Settings').not.toBeNull();
  expect(wide![1]).toMatch(/display:\s*grid/);
  expect(wide![1]).toMatch(/grid-template-columns:\s*repeat\(2,/);
  expect(wide![2]).toMatch(/grid-column:\s*1 \/ -1/);
  // Outside that condition the body is the single column it was.
  const base = rules.filter(
    (rule) =>
      rule.selectors.includes('.settings-body') &&
      !/grid-template-columns/.test(rule.body),
  );
  expect(base).toHaveLength(1);
  expect(base[0].body).toMatch(/flex-direction:\s*column/);
});
