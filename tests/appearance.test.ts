import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
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

it('provides every DN-34 theme with legible text, semantic text, focus and boundaries', () => {
  const themes = cssThemes();
  expect(Object.keys(themes).sort()).toEqual([...APPEARANCE_THEMES].sort());
  for (const theme of APPEARANCE_THEMES) {
    const tokens = themes[theme]!;
    const background = tokens.bg!;
    for (const foreground of [
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
    ]) {
      expect(
        contrast(tokens[foreground]!, background),
        `${theme} ${foreground}`,
      ).toBeGreaterThanOrEqual(4.5);
    }
    expect(
      contrast(tokens['glass-line']!, background),
      `${theme} boundary`,
    ).toBeGreaterThanOrEqual(3);
    expect(
      contrast(tokens['glass-line-strong']!, background),
      `${theme} strong boundary`,
    ).toBeGreaterThanOrEqual(3);
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
