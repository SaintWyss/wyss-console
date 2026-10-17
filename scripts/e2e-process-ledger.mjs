// Process ledger for the native harness. Every process the harness starts is recorded in the
// sandbox's ledger file the moment it is spawned, with its start time from /proc. Stopping
// signals a process group only when its leader still has that start time, so a PID the
// kernel has reused is never signalled, and nothing is found by name.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Start time of a live process, from /proc/<pid>/stat. Null when the process does not exist
 * or is a zombie, so a finished process never counts as owned.
 * @param {number} pid
 * @returns {string | null}
 */
export function identityOf(pid) {
  let stat;
  try {
    stat = readFileSync(`/proc/${pid}/stat`, 'utf8');
  } catch {
    return null;
  }
  // The command name is in parentheses and may hold spaces: fields start after the last ')'.
  // Field 3 (state) is index 0, so field 22 (starttime) is index 19.
  const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
  if (fields[0] === 'Z' || fields[0] === 'X') return null;
  return fields[19];
}

/** @param {string} root @returns {string} */
export function ledgerPath(root) {
  return join(root, 'ledger.json');
}

/**
 * @param {string} root
 * @returns {{ name: string, pid: number, startTime: string }[]}
 */
export function readLedger(root) {
  let content;
  try {
    content = readFileSync(ledgerPath(root), 'utf8');
  } catch (error) {
    if (error?.code === 'ENOENT') return [];
    throw error;
  }
  let entries;
  try {
    entries = JSON.parse(content);
  } catch {
    throw new Error('invalid process ledger: cannot safely clean up');
  }
  if (
    !Array.isArray(entries) ||
    entries.some(
      (entry) =>
        typeof entry?.name !== 'string' ||
        !Number.isSafeInteger(entry?.pid) ||
        entry.pid <= 0 ||
        typeof entry?.startTime !== 'string' ||
        !/^\d+$/.test(entry.startTime),
    )
  ) {
    throw new Error('invalid process ledger: cannot safely clean up');
  }
  return entries;
}

/**
 * Records a process the harness just spawned. Throws when it has already exited, because a
 * process that cannot be identified cannot be stopped safely.
 * @param {string} root
 * @param {string} name
 * @param {number} pid
 * @returns {number} the same PID
 */
export function recordProcess(root, name, pid) {
  const startTime = identityOf(pid);
  if (startTime === null) {
    throw new Error(`${name} exited before it could be recorded`);
  }
  const entries = readLedger(root);
  entries.push({ name, pid, startTime });
  writeFileSync(ledgerPath(root), `${JSON.stringify(entries, null, 2)}\n`);
  return pid;
}

/** @param {number} ms */
const pause = (ms) => new Promise((done) => setTimeout(done, ms));

/**
 * True while the process group `pid` has any member left. The kernel does not reuse a PID
 * while it is still a process group id, so a live group led by a recorded PID is the
 * recorded group.
 * @param {number} pid
 */
function groupAlive(pid) {
  try {
    process.kill(-pid, 0);
    return true;
  } catch {
    return false;
  }
}

/**
 * Sends a signal to a recorded group, if it is still the recorded one: its leader has the
 * recorded start time, or the leader has exited and the group still has members.
 * @param {{ pid: number, startTime: string }} entry
 * @param {NodeJS.Signals} signal
 */
function signalRecorded(entry, signal) {
  const leader = identityOf(entry.pid);
  if (leader !== entry.startTime && leader !== null) return false;
  try {
    process.kill(-entry.pid, signal);
    return true;
  } catch {
    return false;
  }
}

/**
 * Runs a startup sequence. When it fails, every process recorded so far is stopped before
 * the error is rethrown, so a failed start leaves nothing running.
 * @template T
 * @param {string} root
 * @param {() => Promise<T>} work
 * @returns {Promise<T>}
 */
export async function withCleanup(root, work) {
  try {
    return await work();
  } catch (error) {
    await terminateRecorded(root);
    throw error;
  }
}

/**
 * Stops every recorded process of a sandbox, newest first. SIGTERM to each recorded group,
 * SIGKILL after the grace period for those still running.
 * @param {string} root
 * @param {{ graceMs?: number, names?: string[] }} [options] names limits the stop to those
 *   recorded names
 * @returns {Promise<{ name: string, pid: number, outcome: string }[]>} one outcome per entry:
 *   stopped, killed, already-gone, identity-changed or survived
 */
export async function terminateRecorded(root, { graceMs = 3000, names } = {}) {
  const report = [];
  const selected = readLedger(root).filter(
    (entry) => names === undefined || names.includes(entry.name),
  );
  for (const entry of selected.reverse()) {
    const { name, pid, startTime } = entry;
    if (!groupAlive(pid)) {
      report.push({ name, pid, outcome: 'already-gone' });
      continue;
    }
    const leader = identityOf(pid);
    if (leader !== startTime && leader !== null) {
      report.push({ name, pid, outcome: 'identity-changed' });
      continue;
    }
    signalRecorded(entry, 'SIGTERM');
    let waited = 0;
    while (groupAlive(pid) && waited < graceMs) {
      await pause(100);
      waited += 100;
    }
    let outcome = 'stopped';
    if (groupAlive(pid)) {
      signalRecorded(entry, 'SIGKILL');
      outcome = 'killed';
      for (let i = 0; i < 30 && groupAlive(pid); i++) await pause(100);
      if (groupAlive(pid)) outcome = 'survived';
    }
    report.push({ name, pid, outcome });
  }
  return report;
}
