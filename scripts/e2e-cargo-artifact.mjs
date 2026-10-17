// Select the test executable named by the cargo invocation that just finished.
// Scanning target/debug/deps could silently choose a stale E2E seed binary.
import { realpathSync, statSync } from 'node:fs';
import { basename, isAbsolute, relative, resolve } from 'node:path';

/**
 * @param {string} output stdout of cargo test --no-run --message-format=json
 * @param {string} depsDir expected cargo debug/deps directory
 * @returns {string} the executable from this cargo build
 */
export function seederFromCargoOutput(output, depsDir) {
  /** @type {Set<string>} */
  const candidates = new Set();
  let finished = false;
  for (const line of output.split(/\r?\n/)) {
    if (!line.trim().startsWith('{')) continue;
    /** @type {{ reason?: string, success?: boolean, target?: { name?: string, kind?: string[] }, executable?: string }} */
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      throw new Error('invalid cargo JSON output');
    }
    if (message.reason === 'build-finished') {
      if (message.success !== true)
        throw new Error('cargo did not finish successfully');
      finished = true;
    }
    if (
      message.reason !== 'compiler-artifact' ||
      message.target?.name !== 'e2e_seed' ||
      !Array.isArray(message.target?.kind) ||
      !message.target.kind.includes('test')
    ) {
      continue;
    }
    const executable = message.executable;
    if (typeof executable !== 'string' || !isAbsolute(executable)) {
      throw new Error('cargo did not report an absolute E2E seeder executable');
    }
    const path = resolve(executable);
    const rel = relative(resolve(depsDir), path);
    if (
      rel.startsWith('..') ||
      isAbsolute(rel) ||
      !/^e2e_seed-[0-9a-f]+$/.test(basename(path)) ||
      !statSync(path).isFile() ||
      realpathSync(path) !== path
    ) {
      throw new Error('cargo reported an unsafe E2E seeder executable path');
    }
    candidates.add(path);
  }
  if (!finished || candidates.size !== 1) {
    throw new Error(
      'cargo did not identify exactly one current E2E seeder executable',
    );
  }
  return [...candidates][0];
}
