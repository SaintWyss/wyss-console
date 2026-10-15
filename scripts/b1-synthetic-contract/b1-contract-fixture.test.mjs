// Synthetic-only contract checks. Never run a provider or contact actual host resources.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync, linkSync, rmSync, statSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {admitSynthetic, syntheticContentDigest, syntheticOverlayDigest, safePath,
  safeSymlink, exportSynthetic, publishSynthetic, B1ContractError} from './b1-contract-fixture.mjs';

const baseOid = 'a'.repeat(40);
const e = (path, data, mode = '100644') => ({path,kind:'regular',mode,bytes:Buffer.from(data)});
const edit = (path, data) => ({...e(path,data), op:'put', expectedSha256:syntheticContentDigest(Buffer.from(data))});
const admit = (baseEntries=[], selectedOverlay=[]) => admitSynthetic({taskId:'fixture-1',baseOid,baseEntries,
  selectedOverlay,approvedOverlayDigest:syntheticOverlayDigest(selectedOverlay)});
const rejected = (f,code) => assert.throws(f,x => x instanceof B1ContractError && x.code === code);

test('admission accepts explicitly selected tracked changes and untracked files without Git commits', () => {
  const overlay=[edit('src/app.js','tracked-changed'),edit('new.txt','untracked-approved')];
  const snapshot=admit([e('src/app.js','original'),e('other.txt','unchanged')],overlay);
  assert.equal(snapshot.entries.length,3);
  assert.equal(snapshot.entries.find(([p]) => p==='src/app.js')[1].bytes.toString(),'tracked-changed');
  assert.equal(snapshot.entries.find(([p]) => p==='new.txt')[1].bytes.toString(),'untracked-approved');
});
test('unselected tree extras are not automatically imported',()=>{
  const snapshot=admit([e('src/app.js','original')]);
  assert.deepEqual(snapshot.entries.map(([path])=>path),['src/app.js']);
});
test('admission rejects tampered preview, bad base OID and invalid overlay digest',()=>{
  rejected(()=>admitSynthetic({taskId:'t',baseOid:'NOT_AN_OID',baseEntries:[],selectedOverlay:[],approvedOverlayDigest:syntheticOverlayDigest([])}),'INVALID_BASE_OID');
  rejected(()=>admitSynthetic({taskId:'t',baseOid,baseEntries:[],selectedOverlay:[edit('x','a')],approvedOverlayDigest:'0'.repeat(64)}),'OWNER_SELECTION_MISMATCH');
  const malicious={...edit('x','hello'),expectedSha256:'0'.repeat(64)};
  rejected(()=>admit([],[malicious]),'CONTENT_DIGEST_MISMATCH');
});
test('changed executable mode is not the same owner-reviewed selection',()=>{
  const edits=[edit('safe.txt','contents')];
  const digest=syntheticOverlayDigest(edits);
  edits[0].mode='100755';
  rejected(()=>admitSynthetic({taskId:'task',baseOid,baseEntries:[],selectedOverlay:edits,approvedOverlayDigest:digest}),'OWNER_SELECTION_MISMATCH');
});
test('paths reject traversal, .git controls, backslashes and parent file collisions',()=>{
  for(const p of ['/absolute','a/../b','a/./b','a//b','.git/config','A/.GIT/config','a\\b','']) rejected(()=>safePath(p),'INVALID_PATH');
  rejected(()=>admit([e('foo','file'),e('foo/bar','child')]),'PARENT_COLLISION');
});
test('symlink targets stay inside project; cannot include absolute host paths',()=>{
  assert.equal(safeSymlink('src/link','../README.md'),'../README.md');
  for(const t of ['/home/owner/.codex/auth.json','../../../etc/passwd','../outside','x/.git/config'])
    rejected(()=>safeSymlink('link',t),'UNSAFE_SYMLINK');
});
test('original buffer cannot be changed by mutation of an admitted snapshot',()=>{
  const source=Buffer.from('safe-content'); const s=admit([e('a',source)]);
  source.fill(0);
  assert.equal(s.entries[0][1].bytes.toString(),'safe-content');
});
test('real synthetic hardlink copied to independent inode: stage mutation does not modify canary',()=>{
  const root=mkdtempSync(join(tmpdir(),'wyss-b1-node-fixture-'));
  try {
    mkdirSync(join(root,'forbidden')); mkdirSync(join(root,'project')); mkdirSync(join(root,'staged'));
    const signer=join(root,'forbidden','synthetic-signer');
    const link=join(root,'project','hardlink-canary');
    const dest=join(root,'staged','hardlink-canary');
    writeFileSync(signer,'SYNTHETIC_FORBIDDEN_SIGNER'); linkSync(signer,link);
    assert.equal(statSync(link).ino,statSync(signer).ino);
    // Deliberately authorized bytes: copying breaks inode alias, NOT semantic data authorization.
    const snapshot=admit([e('hardlink-canary',readFileSync(link))]);
    writeFileSync(dest,snapshot.entries[0][1].bytes,{flag:'wx'});
    assert.notEqual(statSync(dest).ino,statSync(signer).ino);
    writeFileSync(dest,'edited');
    assert.equal(readFileSync(signer,'utf8'),'SYNTHETIC_FORBIDDEN_SIGNER');
  } finally {rmSync(root,{recursive:true,force:true});}
});
test('exporter rejects malicious alias and special node records',()=>{
  const s=admit([e('main.txt','A')]);
  rejected(()=>exportSynthetic(s,[{path:'docs/link',kind:'symlink',mode:'120000',target:'/home/user/.config'}]),'UNSAFE_SYMLINK');
  rejected(()=>exportSynthetic(s,[{path:'pipe',kind:'fifo',mode:'100644'}]),'UNSUPPORTED_ENTRY');
  rejected(()=>exportSynthetic(s,[e('.git/config','evil')]),'INVALID_PATH');
});
test('deferred execution surfaces are flagged; quarantine never writes to source',()=>{
  const source={main:'keep'};
  const s=admit([e('main.txt',source.main)]);
  const output=exportSynthetic(s,[e('main.txt','new'),e('.github/workflows/ci.yml','echo danger')]);
  assert.deepEqual(output.changes.find(c=>c.path==='.github/workflows/ci.yml').flags,['DEFERRED_EXECUTION_SURFACE']);
  const pending=new Map(); const receipt=publishSynthetic(pending,output);
  assert.equal(receipt.status,'PENDING'); assert.equal(source.main,'keep');
  rejected(()=>publishSynthetic(pending,output),'QUARANTINE_CAS_CONFLICT');
});
test('publisher refuses an altered export payload instead of trusting caller digest',()=>{
  const s=admit([e('a','x')]); const exp=exportSynthetic(s,[e('a','y')]);
  exp.changes[0].bytes[0]=122;
  rejected(()=>publishSynthetic(new Map(),exp),'EXPORT_DIGEST_MISMATCH');
});
test('deleting tracked files becomes a typed deletion, not a host operation',()=>{
  const ov=[{path:'obsolete',op:'delete'}]; const s=admit([e('obsolete','x'),e('main','A')],ov);
  assert.deepEqual(s.entries.map(([p])=>p),['main']);
  const result=exportSynthetic(s,[]);
  assert.deepEqual(result.changes.map(c=>[c.path,c.op]),[['main','delete']]);
});
