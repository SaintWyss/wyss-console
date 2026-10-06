import { spawn } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const [tool, ...args] = process.argv.slice(2);
const cache = join(root, '.cache');
const env = {
  ...process.env,
  CARGO_HOME: join(cache, 'cargo'),
  CARGO_TARGET_DIR: join(root, 'src-tauri', 'target'),
  XDG_CACHE_HOME: join(cache, 'xdg-cache'),
  XDG_CONFIG_HOME: join(cache, 'xdg-config'),
  XDG_DATA_HOME: join(cache, 'xdg-data'),
};
for (const directory of [
  env.CARGO_HOME,
  env.XDG_CACHE_HOME,
  env.XDG_CONFIG_HOME,
  env.XDG_DATA_HOME,
]) {
  mkdirSync(directory, { recursive: true });
}

/** @type {string} */
let command;
/** @type {string[]} */
let commandArgs;
if (tool === 'cargo') {
  command = 'cargo';
  commandArgs = args;
} else if (tool === 'tauri') {
  command = process.execPath;
  commandArgs = [
    join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
    ...args,
  ];
} else if (tool === 'app' && args.length === 0) {
  command = join(root, 'src-tauri', 'target', 'release', 'wyss-console');
  commandArgs = [];
} else {
  throw new Error('Expected cargo, tauri or app');
}

const child = spawn(command, commandArgs, { cwd: root, env, stdio: 'inherit' });
child.on('error', (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
child.on('exit', (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
});
for (const signal of /** @type {const} */ (['SIGINT', 'SIGTERM'])) {
  process.on(signal, () => child.kill(signal));
}
