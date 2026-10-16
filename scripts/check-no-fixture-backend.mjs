// Fails when a packaged product binary carries the Codex test fixture. Run after
// `npm run desktop:package`; it checks the package directory it is given (default
// `.artifacts/package`). Finding no binary is a failure, never a pass.
import { resolve } from 'node:path';
import { fixtureHitsInFile, productBinaries } from './e2e-guard.mjs';

const dir = resolve(process.argv[2] ?? '.artifacts/package');

/** @type {string[]} */
let binaries;
try {
  binaries = productBinaries(dir);
} catch (error) {
  console.error(
    `check-no-fixture-backend: cannot read ${dir}: ${String(error)}`,
  );
  process.exit(2);
}
if (binaries.length === 0) {
  console.error(`check-no-fixture-backend: no product binaries under ${dir}`);
  process.exit(2);
}

let failed = false;
for (const binary of binaries) {
  const hits = fixtureHitsInFile(binary);
  if (hits.length > 0) {
    failed = true;
    console.error(`fixture backend present in ${binary}: ${hits.join(', ')}`);
  } else {
    console.log(`ok    ${binary}: no fixture marker`);
  }
}
process.exit(failed ? 1 : 0);
