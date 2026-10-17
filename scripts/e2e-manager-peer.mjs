// Prove that the private systemd socket is served by the process recorded at launch,
// not a same-UID relay to the user's real service manager. Socket name/type/UID alone
// do not authenticate the listener.
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { identityOf, readLedger } from './e2e-process-ledger.mjs';

const peerScript = fileURLToPath(
  new URL('./e2e-manager-peer.py', import.meta.url),
);

/**
 * @param {string} root
 * @returns {{pid:number, uid:number}} kernel-verified connected socket identity
 */
export function verifyManagerPeer(root) {
  const managers = readLedger(root).filter((entry) => entry.name === 'systemd');
  if (managers.length !== 1) {
    throw new Error('sandbox systemd manager identity is absent or ambiguous');
  }
  const manager = managers[0];
  if (identityOf(manager.pid) !== manager.startTime) {
    throw new Error('sandbox systemd manager process identity changed');
  }
  const result = spawnSync(
    '/usr/bin/python3',
    ['-I', peerScript, join(root, 'run', 'systemd', 'private')],
    {
      encoding: 'utf8',
      timeout: 3000,
      maxBuffer: 4096,
      env: { PATH: '/usr/bin:/bin', LANG: 'C.UTF-8' },
    },
  );
  if (result.error || result.status !== 0) {
    throw new Error('cannot authenticate sandbox systemd manager socket');
  }
  /** @type {{pid?:number,uid?:number}} */
  let peer;
  try {
    peer = JSON.parse(result.stdout);
  } catch {
    throw new Error('invalid sandbox systemd peer credentials');
  }
  if (
    typeof peer.pid !== 'number' ||
    typeof peer.uid !== 'number' ||
    !Number.isSafeInteger(peer.pid) ||
    !Number.isSafeInteger(peer.uid) ||
    peer.pid !== manager.pid ||
    peer.uid !== process.getuid?.() ||
    identityOf(manager.pid) !== manager.startTime
  ) {
    throw new Error(
      'sandbox private manager socket is served by another process',
    );
  }
  return { pid: peer.pid, uid: peer.uid };
}
