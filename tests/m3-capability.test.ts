import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import m3 from '../src-tauri/capabilities/m3.json';
import config from '../src-tauri/tauri.conf.json';

const root = join(__dirname, '..', 'src-tauri');
const read = (path: string) => readFileSync(join(root, path), 'utf8');

it('lets the main window read the providers state and nothing else', () => {
  expect(config.app.security.capabilities).toContain('m3');
  expect(m3.identifier).toBe('m3');
  expect(m3.windows).toEqual(['main']);
  expect(m3.permissions).toEqual(['allow-m3-state']);
  expect(read('build.rs')).toContain('"m3_state"');
});

it('grants no fixture command in any capability the package lists', () => {
  // The fixture binary adds its own grant when it starts (see `grant` in the fixture host); the
  // capability files, which the package is made from, never name one of its five commands.
  for (const file of readdirSync(join(root, 'capabilities'))) {
    expect(read(join('capabilities', file)), file).not.toMatch(
      /m3-fixture|m3_fixture/,
    );
  }
  expect(JSON.stringify(config)).not.toMatch(/m3-fixture|m3_fixture/);
});
