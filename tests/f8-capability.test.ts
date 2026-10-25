import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import main from '../src-tauri/capabilities/main.json';
import f8 from '../src-tauri/capabilities/f8.json';
import config from '../src-tauri/tauri.conf.json';

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

it('splits the interface across the capabilities the installed configuration lists', () => {
  expect(main.permissions.filter((p) => p.startsWith('allow-f8-'))).toEqual([
    'allow-f8-state',
  ]);
  // The rest is its own capability, listed beside `main` in the one configuration. The
  // commands answer `inactive` until the desktop is started with `--f8-authority`, which a
  // window cannot do (see the Rust test of the entry points).
  // `projects` is the project workspace's own capability (see projects-capability.test.ts).
  expect(config.app.security.capabilities).toEqual([
    'main',
    'f8',
    'projects',
    'm3',
  ]);
  expect(f8.permissions).not.toContain('allow-f8-state');
  expect(f8.windows).toEqual(['main']);
});

it('names no control method, process, path or generic call in what it grants', () => {
  for (const name of f8.permissions)
    expect(name, name).toMatch(/^allow-f8-[a-z-]+$/);
  expect(f8.permissions.join(' ')).not.toMatch(
    /exec|shell|spawn|method|argv|env|path|file|fs-/,
  );
});
