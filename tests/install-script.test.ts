/// <reference types="node" />
import { afterEach, beforeEach, expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const script = join(__dirname, '..', 'scripts', 'install.sh');
let root: string;
let home: string;

const sha = (path: string) =>
  createHash('sha256').update(readFileSync(path)).digest('hex');

/** A package as `desktop:package` writes it, with a stand-in for the binaries. */
function makePackage(version: string, body = 'echo wyss-console-fake') {
  const dir = mkdtempSync(join(root, 'pkg-'));
  mkdirSync(join(dir, 'bin'));
  mkdirSync(join(dir, 'libexec', 'wyss-console'), { recursive: true });
  mkdirSync(join(dir, 'share', 'wyss-console'), { recursive: true });
  writeFileSync(
    join(dir, 'bin', 'wyss-console'),
    `#!/bin/sh\ncase "$1" in --wyss-host-stop) echo stopped >> "${root}/stops";; --diagnose) echo "ok   fake ${version}";; *) ${body};; esac\n`,
  );
  writeFileSync(
    join(dir, 'libexec', 'wyss-console', 'wyss-control'),
    `#!/bin/sh\necho authority ${version}\n`,
  );
  for (const f of ['bin/wyss-console', 'libexec/wyss-console/wyss-control'])
    chmodSync(join(dir, f), 0o755);
  writeFileSync(join(dir, 'share', 'wyss-console', 'icon.png'), 'png');
  writeFileSync(
    join(dir, 'share', 'wyss-console', 'package.json'),
    JSON.stringify(
      {
        schema_version: 1,
        name: 'wyss-console',
        version,
        files: {
          'bin/wyss-console': sha(join(dir, 'bin', 'wyss-console')),
          'libexec/wyss-console/wyss-control': sha(
            join(dir, 'libexec', 'wyss-console', 'wyss-control'),
          ),
        },
      },
      null,
      2,
    ),
  );
  return dir;
}
const run = (...args: string[]) =>
  spawnSync('bash', [script, ...args], {
    env: {
      ...process.env,
      HOME: home,
      XDG_STATE_HOME: join(home, '.local', 'state'),
    },
    encoding: 'utf8',
  });
const prefix = () => join(home, '.local', 'opt', 'wyss-console');

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'wyss-install-'));
  home = join(root, 'home');
  mkdirSync(join(home, '.claude'), { recursive: true });
  writeFileSync(join(home, '.claude', 'settings.json'), '{"sentinel":true}');
});
afterEach(() => rmSync(root, { recursive: true, force: true }));

it('installs a verified package for the user only and wires the launcher, the menu entry and the icon', () => {
  const result = run('install', '--package', makePackage('1.0.0'));
  expect(result.status, result.stderr).toBe(0);
  expect(existsSync(join(prefix(), 'bin', 'wyss-console'))).toBe(true);
  expect(readlinkSync(join(home, '.local', 'bin', 'wyss-console'))).toBe(
    join(prefix(), 'bin', 'wyss-console'),
  );
  const entry = readFileSync(
    join(home, '.local', 'share', 'applications', 'wyss-console.desktop'),
    'utf8',
  );
  expect(entry).toContain(`Exec=${join(prefix(), 'bin', 'wyss-console')}`);
  expect(
    existsSync(
      join(
        home,
        '.local',
        'share',
        'icons',
        'hicolor',
        '128x128',
        'apps',
        'wyss-console.png',
      ),
    ),
  ).toBe(true);
  expect(
    JSON.parse(
      readFileSync(
        join(prefix(), 'share', 'wyss-console', 'install.json'),
        'utf8',
      ),
    ).version,
  ).toBe('1.0.0');
  // Nobody but the owner can write in it.
  expect(lstatSync(join(prefix(), 'bin', 'wyss-console')).mode & 0o022).toBe(0);
});

it('refuses a package whose file does not match its digest and installs nothing', () => {
  const pkg = makePackage('1.0.0');
  writeFileSync(join(pkg, 'bin', 'wyss-console'), '#!/bin/sh\necho tampered\n');
  const result = run('install', '--package', pkg);
  expect(result.status).not.toBe(0);
  expect(result.stderr).toContain('does not match its digest');
  expect(existsSync(prefix())).toBe(false);
  expect(existsSync(join(home, '.local', 'bin', 'wyss-console'))).toBe(false);
});

it('upgrades beside the old version, asks the host to stop first, and can roll back', () => {
  expect(run('install', '--package', makePackage('1.0.0')).status).toBe(0);
  expect(run('install', '--package', makePackage('2.0.0')).status).toBe(0);
  expect(readFileSync(join(root, 'stops'), 'utf8')).toContain('stopped');
  const version = () =>
    JSON.parse(
      readFileSync(
        join(prefix(), 'share', 'wyss-console', 'install.json'),
        'utf8',
      ),
    ).version;
  expect(version()).toBe('2.0.0');
  expect(existsSync(`${prefix()}.previous`)).toBe(true);
  expect(run('rollback').status).toBe(0);
  expect(version()).toBe('1.0.0');
  expect(
    JSON.parse(
      readFileSync(
        join(`${prefix()}.previous`, 'share', 'wyss-console', 'install.json'),
        'utf8',
      ),
    ).version,
  ).toBe('2.0.0');
  expect(run('status').stdout).toContain('fake 1.0.0');
});

it('removes the application and keeps the Console’s state unless asked, and never touches the agents’ own configuration', () => {
  expect(
    run('install', '--package', makePackage('1.0.0'), '--host-service').status,
  ).toBe(0);
  const unit = join(
    home,
    '.config',
    'systemd',
    'user',
    'wyss-console-host.service',
  );
  expect(readFileSync(unit, 'utf8')).toContain('--wyss-host');
  expect(readFileSync(unit, 'utf8')).toContain('RestartPreventExitStatus=2');
  const state = join(home, '.local', 'state', 'wyss-console');
  mkdirSync(state, { recursive: true });
  writeFileSync(join(state, 'layout.json'), '{}');
  expect(run('uninstall').status).toBe(0);
  expect(existsSync(prefix())).toBe(false);
  expect(existsSync(unit)).toBe(false);
  expect(existsSync(join(home, '.local', 'bin', 'wyss-console'))).toBe(false);
  expect(existsSync(join(state, 'layout.json'))).toBe(true);
  expect(run('uninstall', '--purge').status).toBe(0);
  expect(existsSync(state)).toBe(false);
  expect(readFileSync(join(home, '.claude', 'settings.json'), 'utf8')).toBe(
    '{"sentinel":true}',
  );
});

it('says what it does not know instead of guessing', () => {
  expect(run('bogus').status).not.toBe(0);
  expect(run('install', '--package', root).stderr).toContain(
    'is not a package',
  );
  expect(run('rollback').stderr).toContain('no previous version');
});
