import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import main from '../src-tauri/capabilities/main.json';
import f8 from '../src-tauri/capabilities/f8.json';
import config from '../src-tauri/tauri.conf.json';
import overlay from '../src-tauri/tauri.f8.conf.json';

const read = (path: string) =>
  readFileSync(join(__dirname, '..', 'src-tauri', path), 'utf8');
const permission = (command: string) => `allow-${command.replace(/_/g, '-')}`;

// The names a command goes by in each of the three places that must agree: the handler the
// application registers, the manifest the build turns into permissions, and the capability
// that grants them. A command missing from the last two is refused by the framework even
// where the first has it, and no test that mocks the bridge would notice.
const handlers = [...read('src/desktop.rs').matchAll(/f8::(f8_[a-z_]+),/g)].map(
  (m) => m[1],
);
const manifest = [...read('build.rs').matchAll(/"(f8_[a-z_]+)"/g)].map(
  (m) => m[1],
);

it('registers, declares and grants the same twenty-four commands', () => {
  expect(handlers).toHaveLength(24);
  expect([...manifest].sort()).toEqual([...handlers].sort());
  const granted = [...main.permissions, ...f8.permissions]
    .filter((p) => p.startsWith('allow-f8-'))
    .sort();
  expect(granted).toEqual(handlers.map(permission).sort());
});

it('grants the installed window one read of the interface and nothing of the rest', () => {
  expect(main.permissions.filter((p) => p.startsWith('allow-f8-'))).toEqual([
    'allow-f8-state',
  ]);
  // The rest is a capability the installed configuration does not list.
  expect(config.app.security.capabilities).toEqual(['main']);
  expect(f8.permissions).not.toContain('allow-f8-state');
  expect(f8.windows).toEqual(['main']);
  // A configuration that activates it lists both, and changes nothing else.
  expect(overlay.app.security.capabilities).toEqual(['main', 'f8']);
  expect(Object.keys(overlay.app)).toEqual(['security']);
});

it('names no control method, process, path or generic call in what it grants', () => {
  for (const name of f8.permissions)
    expect(name, name).toMatch(/^allow-f8-[a-z-]+$/);
  expect(f8.permissions.join(' ')).not.toMatch(
    /exec|shell|spawn|method|argv|env|path|file|fs-/,
  );
});
