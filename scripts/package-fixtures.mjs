// Read-only fixture-marker check, recovered from the unmerged native E2E branch.
// It neither starts that harness nor creates a HOME, bus, provider or service.
// Marker absence is a packaging check, not a security attestation.
import { closeSync, constants, fstatSync, openSync, readSync } from 'node:fs';
import { join } from 'node:path';

export const FIXTURE_MARKERS = [
  'WYSS_RUNTIME_FIXTURE',
  'WYSS_RUNTIME_SCENARIO',
  'codex_runtime.py',
  'wyss-b1-synthetic-',
  'WYSS_M3_FIXTURE_HOST',
  // The real binary's own fixture arguments (feature m3-fixture): a packaged build has none of them.
  '--m3-fixture-sdk-root',
  '--m3-fixture-script',
];
const CHUNK_BYTES = 64 * 1024;
const MAX_BINARY_BYTES = 512 * 1024 * 1024;
const OVERLAP = Math.max(...FIXTURE_MARKERS.map((marker) => marker.length)) - 1;

/**
 * Scan a regular binary with bounded memory. Refuse absent/linked/special/empty/
 * oversized files and observable changes during the scan. Never execute a payload.
 * @param {string} path
 * @returns {string[]}
 */
export function fixtureHitsInFile(path) {
  const fd = openSync(
    path,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  );
  try {
    const before = fstatSync(fd);
    if (
      !before.isFile() ||
      before.size === 0 ||
      before.size > MAX_BINARY_BYTES
    ) {
      throw new Error(
        'Expected a nonempty regular product binary up to 512 MiB',
      );
    }
    const chunk = Buffer.alloc(CHUNK_BYTES);
    const hits = new Set();
    let carry = Buffer.alloc(0);
    let total = 0;
    while (total < before.size) {
      const count = readSync(
        fd,
        chunk,
        0,
        Math.min(chunk.length, before.size - total),
        null,
      );
      if (count === 0) throw new Error('Product binary changed during scan');
      const window = Buffer.concat([carry, chunk.subarray(0, count)]);
      for (const marker of FIXTURE_MARKERS) {
        if (window.includes(Buffer.from(marker, 'ascii'))) hits.add(marker);
      }
      carry = Buffer.from(
        window.subarray(Math.max(0, window.length - OVERLAP)),
      );
      total += count;
    }
    const after = fstatSync(fd);
    if (
      after.size !== before.size ||
      after.mtimeMs !== before.mtimeMs ||
      after.ctimeMs !== before.ctimeMs
    ) {
      throw new Error('Product binary changed during scan');
    }
    return FIXTURE_MARKERS.filter((marker) => hits.has(marker));
  } finally {
    closeSync(fd);
  }
}

/**
 * Inspect exactly the two binaries in one current package; never recurse into old
 * packages or follow a symlink tree searching for something that happens to pass.
 * @param {string} directory
 * @returns {string[]}
 */
export function verifyPackageFixtures(directory) {
  const binaries = [
    join(directory, 'bin', 'wyss-console'),
    join(directory, 'libexec', 'wyss-console', 'wyss-control'),
  ];
  for (const binary of binaries) {
    const hits = fixtureHitsInFile(binary);
    if (hits.length) {
      throw new Error(
        `Fixture backend present in ${binary}: ${hits.join(', ')}`,
      );
    }
  }
  return binaries;
}
