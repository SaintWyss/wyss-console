import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { expect, it } from 'vitest';
import { APPEARANCE_THEMES } from '../src/lib/contracts/settings';
import { terminalPalette } from '../src/lib/features/terminal/appearance';

const stylesheet = readFileSync(resolve(process.cwd(), 'src/app.css'), 'utf8');

function luminance(hex: string): number {
  const channels = hex.match(/[0-9a-f]{2}/gi);
  if (!channels || channels.length !== 3)
    throw new Error(`expected hex color: ${hex}`);
  const [r, g, b] = channels.map((channel) => parseInt(channel, 16) / 255);
  const linear = (value: number) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  return 0.2126 * linear(r!) + 0.7152 * linear(g!) + 0.0722 * linear(b!);
}

function contrast(left: string, right: string): number {
  const a = luminance(left);
  const b = luminance(right);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

function cssThemes(): Record<string, Record<string, string>> {
  const found: Record<string, Record<string, string>> = {};
  const blocks = stylesheet.matchAll(
    /:root(?:\[data-theme=['"]([^'"]+)['"]\])?\s*\{([^}]*)\}/g,
  );
  for (const block of blocks) {
    const theme = block[1] ?? 'dark';
    found[theme] = Object.fromEntries(
      [...block[2]!.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6});/g)].map(
        (match) => [match[1]!, match[2]!],
      ),
    );
  }
  return found;
}

/** Every surface text can sit on. Each must be an opaque color in every theme. */
const SURFACES = [
  'chrome',
  'bg',
  'surface',
  'surface-subtle',
  'surface-control',
  'surface-raised',
  'surface-hover',
  'surface-strong',
  'surface-overlay',
  'accent-soft',
];
const TEXT = [
  'text',
  'muted',
  'faint',
  'accent',
  'accent-strong',
  'ok',
  'pending',
  'warn',
  'error',
  'error-text',
];

it('defines every surface of every theme as an opaque color', () => {
  const themes = cssThemes();
  expect(Object.keys(themes).sort()).toEqual([...APPEARANCE_THEMES].sort());
  for (const theme of APPEARANCE_THEMES)
    for (const surface of [...SURFACES, 'ok-soft', 'warn-soft', 'error-soft'])
      // Only six-digit hex is read as a token: rgba(), transparent or a missing one fail.
      expect(themes[theme]![surface], `${theme} ${surface}`).toMatch(
        /^#[0-9a-f]{6}$/i,
      );
});

it('keeps every text token legible on every surface of its theme, not only the canvas', () => {
  for (const theme of APPEARANCE_THEMES) {
    const tokens = cssThemes()[theme]!;
    for (const surface of SURFACES) {
      for (const foreground of TEXT)
        expect(
          contrast(tokens[foreground]!, tokens[surface]!),
          `${theme} ${foreground} on ${surface}`,
        ).toBeGreaterThanOrEqual(4.5);
      for (const boundary of ['line-strong', 'focus'])
        expect(
          contrast(tokens[boundary]!, tokens[surface]!),
          `${theme} ${boundary} on ${surface}`,
        ).toBeGreaterThanOrEqual(3);
    }
    // A state badge: its word on its own tinted background.
    for (const [text, soft] of [
      ['ok', 'ok-soft'],
      ['warn', 'warn-soft'],
      ['error-text', 'error-soft'],
      ['pending', 'accent-soft'],
      ['text', 'warn-soft'],
    ] as const)
      expect(
        contrast(tokens[text]!, tokens[soft]!),
        `${theme} ${text} on ${soft}`,
      ).toBeGreaterThanOrEqual(4.5);
  }
});

it('keeps a filled Arsenal meter cell distinct from every surface it can sit on, in every theme', () => {
  // The composition meter fills its cells with --accent. A filled cell must stand out from its
  // surface by 3:1 (non-text), and its empty cell by the same border rule as every control.
  for (const theme of APPEARANCE_THEMES) {
    const tokens = cssThemes()[theme]!;
    for (const surface of SURFACES)
      expect(
        contrast(tokens.accent!, tokens[surface]!),
        `${theme} filled meter cell on ${surface}`,
      ).toBeGreaterThanOrEqual(3);
  }
});

it('never lets what is behind the window show through: no blur, no glass mode', () => {
  const svelte = (dir: string): string[] =>
    readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
      entry.isDirectory()
        ? svelte(join(dir, entry.name))
        : entry.name.endsWith('.svelte')
          ? [join(dir, entry.name)]
          : [],
    );
  const sources = [
    stylesheet,
    ...svelte(resolve(process.cwd(), 'src')).map((f) =>
      readFileSync(f, 'utf8'),
    ),
  ];
  for (const source of sources) {
    expect(source).not.toMatch(/backdrop-filter|blur\(|data-glass|glass-/);
    expect(source).not.toMatch(/--(frame|glow-[a-z]+)\b/);
  }
  // No background anywhere is see-through or fades out: no rgba/hsla, no gradient, no alpha hex.
  // `transparent` alone stays allowed: it shows its opaque parent, never the desktop.
  for (const source of sources)
    for (const [, value] of source.matchAll(
      /background(?:-color|-image)?\s*:\s*([^;}]+)/g,
    )) {
      expect(value, `background: ${value}`).not.toMatch(
        /rgba\(|hsla\(|gradient\(|#[0-9a-f]{8}\b|#[0-9a-f]{4}\b/i,
      );
      expect(value, `background: ${value}`).not.toMatch(/\/\s*0?\.\d/);
    }
  // No text below the 12 px floor of the type scale.
  for (const source of sources)
    for (const [, size] of source.matchAll(
      /font(?:-size)?\s*:\s*(\d+(?:\.\d+)?)px/g,
    ))
      expect(Number(size), `font ${size}px`).toBeGreaterThanOrEqual(12);
  // A selected row is marked by shape as well as tint.
  for (const row of ['session-row', 'event-row'])
    expect(stylesheet).toMatch(
      new RegExp(
        `\\.${row}\\[aria-pressed='true'\\]\\s*\\{[^}]*box-shadow:\\s*inset 3px 0 0 var\\(--accent\\);`,
        's',
      ),
    );
  // The window itself is created opaque, and nothing asks the compositor for blur.
  const desktop = readFileSync(
    resolve(process.cwd(), 'src-tauri/src/desktop.rs'),
    'utf8',
  );
  expect(desktop).toMatch(/\.transparent\(false\)/);
  expect(desktop).not.toMatch(/mod glass|BLUR_BEHIND|\.transparent\(true\)/);
  expect(
    readdirSync(resolve(process.cwd(), 'src-tauri/src/desktop')),
  ).not.toContain('glass.rs');
  // The shell and its chrome paint opaque tokens.
  expect(stylesheet).toMatch(/\.shell\s*\{[^}]*background:\s*var\(--bg\);/s);
  expect(stylesheet).toMatch(
    /\.top-bar\s*\{[^}]*background:\s*var\(--chrome\);/s,
  );
  expect(stylesheet).toMatch(
    /\.sidebar\s*\{[^}]*background:\s*var\(--chrome\);/s,
  );
});

it('keeps the keyboard skip link readable over the window', () => {
  expect(stylesheet).toMatch(
    /\.skip-link\s*\{[^}]*color:\s*var\(--text\);[^}]*border:\s*2px solid var\(--accent\);[^}]*background:\s*var\(--bg\);/s,
  );
  for (const theme of APPEARANCE_THEMES) {
    const tokens = cssThemes()[theme]!;
    expect(
      contrast(tokens.text!, tokens.bg!),
      `${theme} skip-link text`,
    ).toBeGreaterThanOrEqual(4.5);
    expect(
      contrast(tokens.accent!, tokens.bg!),
      `${theme} skip-link boundary`,
    ).toBeGreaterThanOrEqual(3);
  }
});

it('keeps light-theme faint text legible on event-row surfaces and states', () => {
  const tokens = cssThemes().light!;
  const surfaceTokens = Object.keys(tokens).filter(
    (name) =>
      name === 'bg' ||
      name === 'surface' ||
      name.startsWith('surface-') ||
      name === 'accent-soft',
  );

  expect(surfaceTokens).toContain('surface-subtle');
  expect(surfaceTokens).toContain('surface-raised');
  expect(stylesheet).toMatch(/\.event-time\s*\{[^}]*color:\s*var\(--faint\);/s);
  expect(stylesheet).toMatch(
    /\.event-row:hover\s*\{[^}]*background:\s*var\(--surface-raised\);/s,
  );
  expect(stylesheet).toMatch(
    /\.event-row\[aria-pressed='true'\]\s*\{[^}]*var\(--accent-soft\)/s,
  );

  for (const surface of surfaceTokens) {
    expect(
      contrast(tokens.faint!, tokens[surface]!),
      `light faint text on ${surface}`,
    ).toBeGreaterThanOrEqual(4.5);
  }
});

it('keeps every terminal ANSI color at 4.5:1 against its selected surface', () => {
  for (const theme of APPEARANCE_THEMES) {
    const palette = terminalPalette(theme);
    for (const [name, foreground] of Object.entries(palette)) {
      if (name === 'background') continue;
      expect(
        contrast(foreground, palette.background),
        `${theme} terminal ${name}`,
      ).toBeGreaterThanOrEqual(4.5);
    }
  }
});
