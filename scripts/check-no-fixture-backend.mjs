// Check a current package without running any application or provider.
import { arch } from 'node:os';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { verifyPackageFixtures } from './package-fixtures.mjs';

const { version } = JSON.parse(
  readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
);
const directory = resolve(
  process.argv[2] ??
    `.artifacts/package/wyss-console-${version}-linux-${arch()}`,
);
try {
  for (const binary of verifyPackageFixtures(directory)) {
    console.log(`ok    ${binary}: no fixture marker`);
  }
} catch (error) {
  console.error(`check-no-fixture-backend: ${String(error)}`);
  process.exitCode = 1;
}
