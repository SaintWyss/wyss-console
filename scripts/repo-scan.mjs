// Scans the tracked files of a checkout for credentials: no credential file by name and
// no credential-shaped content in any of them, historical documents included. There is no
// exception: a file that holds something that looks like a credential is changed, not
// excused.
//
//   node scripts/repo-scan.mjs [--repo-root <dir>]
//
// What is scanned is what Git records (the committed tree and the index, read from Git's
// objects) and what is on disk for each tracked path. A tracked file that is missing from
// the working tree, is not the kind of entry Git records, or cannot be read is a failure,
// never a file that was skipped: it is still scanned as it was committed.
//
// Exit status 0 only when every check passed. Nothing a check finds is printed as a
// value: a finding names a file and a kind.
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { credentialFiles, scanCheckout } from './repo-scan-lib.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

/** @type {string[]} */
const problems = [];
/** @type {string[]} */
const passed = [];
/**
 * @param {boolean} ok
 * @param {string} what
 * @param {string} [why]
 */
function check(ok, what, why = '') {
  if (ok) passed.push(what);
  else problems.push(why ? `${what}: ${why}` : what);
}

const argv = process.argv.slice(2);
const at = argv.indexOf('--repo-root');
const where = resolve(
  at !== -1 && argv[at + 1] ? /** @type {string} */ (argv[at + 1]) : root,
);

const scan = scanCheckout(where);
check(scan.paths.length > 0, 'the checkout has tracked files to scan');
check(
  scan.problems.length === 0,
  'every tracked file was read, from Git and from the working tree',
  scan.problems.map((p) => `${p.path} (${p.problem})`).join(', '),
);
const tracked = credentialFiles(scan.paths);
check(
  tracked.length === 0,
  'no credential file is tracked',
  tracked.map((f) => `${f.path} (${f.kind})`).join(', '),
);
check(
  scan.secrets.length === 0,
  `no credential in any of the ${scan.paths.length} tracked files`,
  scan.secrets.map((s) => `${s.path} (${s.kind})`).join(', '),
);

for (const line of passed) console.log(`ok    ${line}`);
for (const line of problems) console.log(`FAIL  ${line}`);
console.log(`\nrepo-scan: ${passed.length} passed, ${problems.length} failed`);
process.exit(problems.length === 0 ? 0 : 1);
