import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {EventEmitter} from 'node:events';
import {relativePath,publicUrl,validateOperation,fileOperation,safePath,terminal,OUTPUT_CAP} from './policy.mjs';
test('paths never escape or select host resources',() => {
  for (const bad of ['../a','a/../b','/etc/passwd','C:\\secret','a\\b','a//b','a\0b']) assert.throws(() => relativePath(bad));
  assert.equal(relativePath('notes/a.txt'),'notes/a.txt');
});
test('URLs and operation identities are constrained',() => {
  for (const bad of ['file:///etc/passwd','http://localhost','http://127.0.0.1','http://2130706433','http://169.254.169.254','http://10.0.0.1','http://[::1]','http://host.docker.internal:2375','https://u:p@example.com','https://example.com:9000']) assert.throws(() => publicUrl(bad));
  assert.equal(publicUrl('https://example.com'),'https://example.com/');
  for (const op of [{action:'terminal',command:'x',agentId:'other'},{action:'read'},{action:'click',x:-1,y:0},{action:'click',x:0,y:640},{action:'type',text:'x'.repeat(16001)},{action:'host_exec'},{action:'key',key:'F99'}]) assert.throws(() => validateOperation(op));
});
test('files are real bounded UTF-8 data in the workspace',async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(),'roadeep-computer-policy-'));
  try {
    await fileOperation({action:'write',path:'notes/a.txt',text:'hello فارسی'},root);
    assert.equal((await fileOperation({action:'read',path:'notes/a.txt'},root)).text,'hello فارسی');
    assert.equal((await fileOperation({action:'list',path:'notes'},root)).files[0].name,'a.txt');
    await fs.writeFile(path.join(root,'large'),Buffer.alloc(OUTPUT_CAP+1));
    await assert.rejects(fileOperation({action:'read',path:'large'},root),/file-limit/);
    await fs.writeFile(path.join(root,'binary'),Buffer.from([0xff,0xfe]));
    await assert.rejects(fileOperation({action:'read',path:'binary'},root));
    await assert.rejects(safePath('../escape',{root}));
  } finally { await fs.rm(root,{recursive:true,force:true}); }
});
test('symlink traversal is rejected',async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(),'roadeep-computer-link-'));
  try {
    try { await fs.symlink(os.tmpdir(),path.join(root,'link'),'junction'); } catch (e) { if (e.code === 'EPERM') { t.skip('OS denies symlink creation'); return; } throw e; }
    await assert.rejects(safePath('link/secret',{root}),/symlink-refused/);
  } finally { await fs.rm(root,{recursive:true,force:true}); }
});
function fakeProcess() { const p = new EventEmitter(); p.pid = 4242; p.stdout = new EventEmitter(); p.stderr = new EventEmitter(); return p; }
test('terminal process group is killed on timeout, overflow and cancellation',async () => {
  let kills = []; let child = fakeProcess();
  const settings = {spawnProcess:() => child,killGroup:pid => kills.push(pid),deadlineMs:10};
  await assert.rejects(terminal('sleep',{...settings}),/terminal-timeout/); assert.deepEqual(kills,[4242]);
  kills = []; child = fakeProcess(); const overflow = terminal('large',{...settings,deadlineMs:100}); child.stdout.emit('data',Buffer.alloc(OUTPUT_CAP+1));
  await assert.rejects(overflow,/output-limit/); assert.deepEqual(kills,[4242]);
  kills = []; child = fakeProcess(); const ctl = new AbortController(); const cancelled = terminal('x',{...settings,deadlineMs:100,signal:ctl.signal}); ctl.abort();
  await assert.rejects(cancelled,/cancelled/); assert.deepEqual(kills,[4242]);
});
test('terminal command executes only in Linux process group with fixed environment',async () => {
  const child = fakeProcess(); let call; let killed = false;
  const result = terminal('printf hi',{spawnProcess:(...args) => { call = args; return child; },killGroup:() => {killed=true;}});
  child.stdout.emit('data',Buffer.from('hi')); child.emit('exit',0);
  assert.deepEqual(await result,{exitCode:0,stdout:'hi',stderr:''});
  assert.equal(call[0],'/bin/sh'); assert.deepEqual(call[1],['-c','printf hi']); assert.equal(call[2].detached,true); assert.equal(call[2].cwd,'/workspace'); assert.equal(call[2].env.DOCKER_HOST,undefined); assert.ok(killed);
});
