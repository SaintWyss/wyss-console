/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import appInfo from './fixtures/app-info-v1.json';
import tauriConfig from '../src-tauri/tauri.conf.json';

const root = join(__dirname, '..');
const read = (file: string) => readFileSync(join(root, file), 'utf8');

it('lays the package out as the desktop looks for its authority', () => {
  // The desktop's own rule: from its executable, one `parent().parent()`, then
  // `libexec/wyss-console/wyss-control`. Kept in step by reading the code that applies it.
  const rust = read('src-tauri/src/lifecycle/package.rs');
  expect(rust).toMatch(/parent\(\)\?\s*\.parent\(\)\?/);
  expect(rust).toMatch(
    /\.join\("libexec"\)\s*\.join\("wyss-console"\)\s*\.join\("wyss-control"\)/,
  );
  // The packager puts the two programs exactly there, below one prefix.
  const packager = read('scripts/package.mjs');
  expect(packager).toContain("[desktop, 'bin/wyss-console']");
  expect(packager).toContain(
    "[authority, 'libexec/wyss-console/wyss-control']",
  );
});

it('bundles no installer, no updater and no signing claim', () => {
  const bundle = tauriConfig.bundle as Record<string, unknown>;
  expect(bundle.active).toBe(false);
  // No installer format, resource, sidecar or updater: the package is the desktop and the
  // authority, and nothing is updated behind the user's back.
  for (const key of [
    'targets',
    'linux',
    'resources',
    'externalBin',
    'createUpdaterArtifacts',
    'licenseFile',
    'fileAssociations',
  ])
    expect(bundle[key], key).toBeUndefined();
  // Signing is claimed nowhere in the configuration.
  expect(JSON.stringify(tauriConfig)).not.toMatch(/sign|pubkey|updater/i);
  // The window is granted what it was, and the project workspace's capability, whose commands
  // answer `unsupported_capability` until the owner turns it on in a native confirmation.
  expect(
    (tauriConfig.app.security as { capabilities: string[] }).capabilities,
  ).toEqual(['main', 'f8', 'projects']);
});

it('opens in a window wide enough for a full-width shell greeting', () => {
  // Measured on CachyOS at the default 16 px font: a 960 px window gave the terminal 70
  // columns, and the distribution's fastfetch greeting prints lines of up to 119. Each
  // column is about 9.6 px, so 1440 px gives 119 or more.
  const [main] = tauriConfig.app.windows;
  expect(main.width).toBe(1440);
  expect(main.height).toBe(810);
  // It can still be made as narrow as before.
  expect(main.minWidth).toBe(320);
  expect(main.minHeight).toBe(320);
});

it('has one name, everywhere it is shown', () => {
  // The window and the top bar said "Wyss AI" while the package, the menu entry and
  // Settings said "Wyss Console".
  expect(tauriConfig.productName).toBe('Wyss Console');
  expect(tauriConfig.app.windows[0].title).toBe(tauriConfig.productName);
  expect(appInfo.name).toBe(tauriConfig.productName);
  expect(read('src/lib/shell/TopBar.svelte')).toContain(
    `<h1>${tauriConfig.productName}</h1>`,
  );
  expect(read('index.html')).toContain(
    `<title>${tauriConfig.productName}</title>`,
  );
});

it('has one version, everywhere it is written', () => {
  const pkg = JSON.parse(read('package.json'));
  const cargo = /^version\s*=\s*"([^"]+)"/m.exec(
    read('src-tauri/Cargo.toml'),
  )?.[1];
  const lock = /name = "wyss-console"\nversion = "([^"]+)"/.exec(
    read('src-tauri/Cargo.lock'),
  )?.[1];
  const npmLock = JSON.parse(read('package-lock.json'));
  expect(tauriConfig.version).toBe(pkg.version);
  expect(cargo).toBe(pkg.version);
  expect(lock).toBe(pkg.version);
  expect(npmLock.version).toBe(pkg.version);
  expect(npmLock.packages[''].version).toBe(pkg.version);
  expect(appInfo.version).toBe(pkg.version);
  // The 0.x line: it is not 1.0.
  expect(pkg.version).toMatch(/^0\.\d+\.\d+$/);
});

it('does not deny what the product has', () => {
  const readme = read('README.md');
  expect(readme).not.toContain('No agent SDK, authority or agent UI');
  expect(readme).not.toContain('Codex is not usable through the Console UI');
});
