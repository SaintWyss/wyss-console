// Synthetic-only, pure-data B1 admission and quarantine contract reference.
// NOT an OS sandbox, trusted approver, provider integration or production implementation.
import { createHash } from 'node:crypto';

const KINDS = new Set(['regular', 'symlink']);
const MODES = new Set(['100644', '100755', '120000']);
export class B1ContractError extends Error {
  constructor(code) { super(code); this.name = 'B1ContractError'; this.code = code; }
}
const fail = (code) => { throw new B1ContractError(code); };
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');

export function safePath(path) {
  if (typeof path !== 'string' || path.length === 0 || path.length > 4096 ||
      path.startsWith('/') || path.includes('\\') || path.includes('\0')) fail('INVALID_PATH');
  const segments = path.split('/');
  if (segments.length > 32 || segments.some(s => s === '' || s === '.' || s === '..' ||
      s.length > 255 || s.toLowerCase() === '.git')) fail('INVALID_PATH');
  return path;
}
export function safeSymlink(path, target) {
  safePath(path);
  if (typeof target !== 'string' || target.length === 0 || target.length > 4096 ||
      target.startsWith('/') || target.includes('\\') || target.includes('\0')) fail('UNSAFE_SYMLINK');
  const segments = path.split('/').slice(0, -1);
  for (const segment of target.split('/')) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') { if (!segments.length) fail('UNSAFE_SYMLINK'); segments.pop(); }
    else { if (segment.toLowerCase() === '.git') fail('UNSAFE_SYMLINK'); segments.push(segment); }
  }
  return target;
}
function entryFromRaw(input, path) {
  if (!input || typeof input !== 'object' || !KINDS.has(input.kind) || !MODES.has(input.mode)) fail('UNSUPPORTED_ENTRY');
  if (input.kind === 'regular' && !['100644','100755'].includes(input.mode)) fail('UNSUPPORTED_ENTRY');
  if (input.kind === 'symlink' && input.mode !== '120000') fail('UNSUPPORTED_ENTRY');
  let data;
  if (input.kind === 'regular') {
    if (!(input.bytes instanceof Uint8Array) || input.bytes.byteLength > 2097152) fail('UNSUPPORTED_ENTRY');
    data = Buffer.from(input.bytes); // independent copy, no mutable reference to the caller
  } else {
    safeSymlink(path, input.target);
    data = Buffer.from(input.target, 'utf8');
  }
  return Object.freeze({ path, kind: input.kind, mode: input.mode,
    size: data.length, sha256: sha(data), bytes: data, ...(input.kind === 'symlink' ? { target: input.target } : {}) });
}
function overlayFields(edits) {
  return edits.map(e => [e.path,e.op,e.kind ?? null,e.mode ?? null,e.expectedSha256 ?? null,e.target ?? null]);
}
function encode(entries) {
  // Stable encoding for this fixture only. Not production canonicalization or approval proof.
  return Buffer.from(JSON.stringify(entries.map(e => [e.path,e.kind,e.mode,e.size,e.sha256])));
}
function checkWithin(entries) {
  const paths = new Set();
  for (const [path,entry] of entries) {
    safePath(path);
    if (paths.has(path)) fail('DUPLICATE_PATH');
    paths.add(path);
    if (entry.kind === 'symlink') safeSymlink(path,entry.target);
  }
  for (const p of paths) {
    let parent = p;
    while (parent.includes('/')) {
      parent = parent.slice(0, parent.lastIndexOf('/'));
      if (paths.has(parent)) fail('PARENT_COLLISION');
    }
  }
}
export function admitSynthetic({ taskId, baseOid, baseEntries, selectedOverlay = [], approvedOverlayDigest }) {
  if (typeof taskId !== 'string' || !/^[a-zA-Z0-9_-]{1,64}$/.test(taskId)) fail('INVALID_TASK');
  if (typeof baseOid !== 'string' || !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(baseOid)) fail('INVALID_BASE_OID');
  if (!Array.isArray(baseEntries) || !Array.isArray(selectedOverlay)) fail('INVALID_MANIFEST');
  if (baseEntries.length + selectedOverlay.length > 2048) fail('CAPACITY_EXCEEDED');
  const snapshot = new Map();
  for (const e of baseEntries) {
    const path = safePath(e.path);
    if (snapshot.has(path)) fail('DUPLICATE_PATH');
    snapshot.set(path, entryFromRaw(e, path));
  }
  for (const edit of selectedOverlay) {
    const path = safePath(edit.path);
    if (!['put', 'delete'].includes(edit.op)) fail('INVALID_OPERATION');
    if (edit.op === 'delete') {
      if (!snapshot.has(path)) fail('INVALID_DELETE');
      snapshot.delete(path);
    } else {
      if (typeof edit.expectedSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(edit.expectedSha256)) fail('MISSING_OWNER_DIGEST');
      const newEntry = entryFromRaw(edit, path);
      if (edit.expectedSha256 !== newEntry.sha256) fail('CONTENT_DIGEST_MISMATCH');
      snapshot.set(path, newEntry);
    }
  }
  const entries = [...snapshot.entries()].sort(([a],[b]) => a < b ? -1 : a > b ? 1 : 0);
  if (entries.reduce((sum,[,v])=>sum+v.size,0) > 16*1024*1024) fail('CAPACITY_EXCEEDED');
  checkWithin(entries);
  const overlayDigest = sha(Buffer.from(JSON.stringify(overlayFields(selectedOverlay))));
  if (approvedOverlayDigest !== overlayDigest) fail('OWNER_SELECTION_MISMATCH');
  const manifestSha256 = sha(encode(entries.map(([,v]) => v)));
  return Object.freeze({ taskId, baseOid, entries, manifestSha256, overlayDigest });
}
export function syntheticOverlayDigest(edits) {
  return sha(Buffer.from(JSON.stringify(overlayFields(edits))));
}
export function syntheticContentDigest(bytes) { return sha(bytes); }

export function exportSynthetic(admission, workerEntries) {
  if (!admission || !Array.isArray(admission.entries) || !Array.isArray(workerEntries)) fail('INVALID_EXPORT');
  if (workerEntries.length > 2048) fail('CAPACITY_EXCEEDED');
  const out = new Map();
  for (const raw of workerEntries) {
    const path = safePath(raw.path);
    if (out.has(path)) fail('DUPLICATE_PATH');
    out.set(path, entryFromRaw(raw,path));
  }
  const entries = [...out.entries()].sort(([a],[b]) => a < b ? -1 : a > b ? 1 : 0);
  checkWithin(entries);
  const old = new Map(admission.entries);
  const changes = [];
  for (const [path,newEntry] of entries) {
    const previous = old.get(path);
    if (!previous || previous.sha256 !== newEntry.sha256 || previous.mode !== newEntry.mode || previous.kind !== newEntry.kind) {
      const flags = [];
      if (/^(?:\.github|\.vscode|\.claude|\.codex|\.husky)\//.test(path) ||
          /^(?:AGENTS\.md|CLAUDE\.md|Makefile|\.envrc|\.gitmodules|\.gitattributes|\.lfsconfig|package\.json)$/.test(path)) flags.push('DEFERRED_EXECUTION_SURFACE');
      if (newEntry.mode === '100755') flags.push('EXECUTABLE');
      if (newEntry.kind === 'symlink') flags.push('SYMLINK');
      changes.push(Object.freeze({path, op:'put', kind:newEntry.kind, mode:newEntry.mode, sha256:newEntry.sha256, bytes:Buffer.from(newEntry.bytes), flags}));
    }
    old.delete(path);
  }
  for (const [path] of old) changes.push(Object.freeze({path,op:'delete'}));
  changes.sort((a,b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  return Object.freeze({taskId:admission.taskId,baseOid:admission.baseOid,admissionDigest:admission.manifestSha256,changes,exportDigest:sha(Buffer.from(JSON.stringify(changes.map(c => [c.path,c.op,c.sha256 ?? null]))))});
}
export function publishSynthetic(quarantine, exportPacket, expectedAbsent = true) {
  if (!(quarantine instanceof Map) || !exportPacket || !Array.isArray(exportPacket.changes)) fail('INVALID_PUBLICATION');
  if (typeof exportPacket.taskId !== 'string' ||
      typeof exportPacket.baseOid !== 'string' ||
      !/^[a-f0-9]{64}$/.test(exportPacket.admissionDigest ?? '') ||
      !/^[a-f0-9]{64}$/.test(exportPacket.exportDigest ?? '')) fail('INVALID_PUBLICATION');
  const observedPaths = new Set();
  const recompute = [];
  for (const change of exportPacket.changes) {
    safePath(change.path);
    if (observedPaths.has(change.path)) fail('DUPLICATE_PATH');
    observedPaths.add(change.path);
    if (change.op === 'put') {
      const checked = entryFromRaw(change, change.path);
      if (checked.sha256 !== change.sha256) fail('EXPORT_DIGEST_MISMATCH');
    } else if (change.op !== 'delete') fail('INVALID_OPERATION');
    recompute.push([change.path,change.op,change.sha256 ?? null]);
  }
  recompute.sort((a,b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);
  if (sha(Buffer.from(JSON.stringify(recompute))) !== exportPacket.exportDigest) fail('EXPORT_DIGEST_MISMATCH');
  checkWithin(exportPacket.changes.filter(c => c.op === 'put').map(c=>[c.path,c]));
  const key = `${exportPacket.taskId}:${exportPacket.exportDigest}`;
  if (expectedAbsent && quarantine.has(exportPacket.taskId)) fail('QUARANTINE_CAS_CONFLICT');
  // Do not give caller mutable pointers into stored content.
  const stored = Object.freeze({ ...exportPacket, status:'PENDING',
    changes: exportPacket.changes.map(c => Object.freeze({...c,...(c.bytes ? {bytes:Buffer.from(c.bytes)} : {})})) });
  quarantine.set(exportPacket.taskId, stored);
  return Object.freeze({taskId:exportPacket.taskId,status:'PENDING',resultKey:key});
}
