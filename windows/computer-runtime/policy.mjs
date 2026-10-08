import fs from 'node:fs/promises';
import path from 'node:path';
import { constants } from 'node:fs';
import { spawn } from 'node:child_process';
export const WORKSPACE = '/workspace';
export const INPUT_CAP = 160 * 1024;
export const OUTPUT_CAP = 64 * 1024;
export function relativePath(raw = '.') {
  if (typeof raw !== 'string' || raw.length > 512 || !raw || /[\\:\x00-\x1f\x7f]/.test(raw) || raw.startsWith('/') || raw.split('/').some(p => !p || p === '..')) throw new Error('computer-invalid-path');
  return raw;
}
export function publicUrl(raw) {
  if (typeof raw !== 'string' || raw.length > 4096) throw new Error('computer-invalid-url');
  const u = new URL(raw);
  if (!['http:', 'https:'].includes(u.protocol) || u.username || u.password || (u.port && !['80', '443'].includes(u.port))) throw new Error('computer-invalid-url');
  // Native transport additionally resolves and pins only public global addresses.
  if (u.hostname === 'localhost' || u.hostname.endsWith('.localhost') || u.hostname.endsWith('.local') || !u.hostname.includes('.')) throw new Error('computer-invalid-url');
  if (/^\d+\.\d+\.\d+\.\d+$/.test(u.hostname)) {
    const [a,b,c] = u.hostname.split('.').map(Number);
    if (a === 0 || a === 10 || a === 127 || a >= 224 || (a === 169 && b === 254) || (a === 172 && b >= 16 && b <= 31) || (a === 192 && (b === 168 || b === 0)) || (a === 100 && b >= 64 && b <= 127) || (a === 198 && (b === 18 || b === 19 || (b === 51 && c === 100))) || (a === 203 && b === 0 && c === 113)) throw new Error('computer-invalid-url');
  }
  return u.toString();
}
export function validateOperation(op) {
  if (!op || typeof op !== 'object' || Array.isArray(op)) throw new Error('computer-invalid-operation');
  const fields = { status: [], screenshot: [], navigate: ['url'], click: ['x','y'], type: ['text'], key: ['key'], scroll: ['deltaY'], list: ['path'], read: ['path'], write: ['path','text'], terminal: ['command'] }[op.action];
  if (!fields || Object.keys(op).some(k => k !== 'action' && !fields.includes(k))) throw new Error('computer-invalid-operation');
  const text = (key, cap) => { if (typeof op[key] !== 'string' || Buffer.byteLength(op[key]) > cap || op[key].includes('\0')) throw new Error('computer-invalid-operation'); };
  if (op.action === 'navigate') publicUrl(op.url);
  if (['list','read','write'].includes(op.action)) { if (op.action !== 'list' && typeof op.path !== 'string') throw new Error('computer-invalid-path'); relativePath(op.path ?? '.'); }
  if (op.action === 'write') text('text', OUTPUT_CAP);
  if (op.action === 'type') text('text', 16000);
  if (op.action === 'terminal') { text('command', 4000); if (!op.command.trim()) throw new Error('computer-invalid-operation'); }
  if (op.action === 'click' && (!Number.isInteger(op.x) || !Number.isInteger(op.y) || op.x < 0 || op.y < 0 || op.x >= 1024 || op.y >= 640)) throw new Error('computer-invalid-operation');
  if (op.action === 'scroll' && (!Number.isInteger(op.deltaY) || Math.abs(op.deltaY) > 4000)) throw new Error('computer-invalid-operation');
  if (op.action === 'key' && !['Enter','Tab','Escape','Backspace','ArrowUp','ArrowDown','ArrowLeft','ArrowRight','Control+A','Control+C','Control+V','Delete','Home','End','PageUp','PageDown'].includes(op.key)) throw new Error('computer-invalid-operation');
  return op;
}
export async function safePath(raw, { root = WORKSPACE, createParents = false } = {}) {
  const rel = relativePath(raw); const target = path.resolve(root, rel);
  if (target !== root && !target.startsWith(root + path.sep)) throw new Error('computer-invalid-path');
  const pieces = rel.split('/').filter(p => p !== '.'); let current = root;
  for (let i = 0; i < pieces.length; i++) {
    current = path.join(current, pieces[i]);
    let stat;
    try { stat = await fs.lstat(current); }
    catch (err) {
      if (err.code !== 'ENOENT') throw err;
      if (i < pieces.length - 1 && createParents) { await fs.mkdir(current, { mode: 0o700 }); stat = await fs.lstat(current); }
      else if (i < pieces.length - 1) throw new Error('computer-file-missing');
    }
    if (stat?.isSymbolicLink()) throw new Error('computer-symlink-refused');
    if (i < pieces.length - 1 && stat && !stat.isDirectory()) throw new Error('computer-invalid-path');
  }
  return target;
}
export async function fileOperation(op, root = WORKSPACE) {
  const target = await safePath(op.path ?? '.', {root,createParents:op.action === 'write'});
  if (op.action === 'list') {
    const dir = await fs.opendir(target); const files = []; let truncated = false;
    try { for await (const entry of dir) { if (files.length === 100) { truncated = true; break; } files.push({name:entry.name.slice(0,512),kind:entry.isSymbolicLink() ? 'symlink' : entry.isDirectory() ? 'directory' : 'file'}); } }
    finally { /* for-await closes the directory even on break. */ }
    return {files,truncated};
  }
  const file = await fs.open(target, op.action === 'write' ? constants.O_WRONLY | constants.O_CREAT | constants.O_TRUNC | constants.O_NOFOLLOW : constants.O_RDONLY | constants.O_NOFOLLOW, 0o600);
  try {
    if (!(await file.stat()).isFile()) throw new Error('computer-not-text-file');
    if (op.action === 'write') { await file.writeFile(op.text, 'utf8'); return {path:op.path,bytes:Buffer.byteLength(op.text)}; }
    if ((await file.stat()).size > OUTPUT_CAP) throw new Error('computer-file-limit');
    const bytes = Buffer.alloc(OUTPUT_CAP + 1); const {bytesRead} = await file.read(bytes,0,bytes.length,0);
    if (bytesRead > OUTPUT_CAP) throw new Error('computer-file-limit');
    const text = new TextDecoder('utf-8',{fatal:true}).decode(bytes.subarray(0,bytesRead));
    return {path:op.path,text};
  } finally { await file.close(); }
}
export function terminal(command, {cwd = WORKSPACE, deadlineMs = 15000, signal, spawnProcess = spawn, killGroup = pid => process.kill(-pid,'SIGKILL')} = {}) {
  return new Promise((resolve,reject) => {
    const child = spawnProcess('/bin/sh',['-c',command], {cwd,detached:true,env:{PATH:'/usr/local/bin:/usr/bin:/bin',HOME:'/tmp/home',LANG:'C.UTF-8'},stdio:['ignore','pipe','pipe']});
    let bytes = 0, out = '', err = '', settled = false;
    const kill = () => { if (child.pid) { try { killGroup(child.pid); } catch (e) { if (e.code !== 'ESRCH') console.error('computer: process cleanup failed'); } } };
    const finish = (error, code) => { if (settled) return; settled = true; clearTimeout(timer); signal?.removeEventListener('abort',cancel); kill(); error ? reject(error) : resolve({exitCode:code,stdout:out,stderr:err}); };
    const cancel = () => finish(new Error('computer-cancelled'));
    const timer = setTimeout(() => finish(new Error('computer-terminal-timeout')),deadlineMs);
    for (const [stream,key] of [[child.stdout,'out'],[child.stderr,'err']]) stream.on('data',chunk => { bytes += chunk.length; if (bytes > OUTPUT_CAP) { finish(new Error('computer-output-limit')); return; } if (key === 'out') out += chunk.toString('utf8'); else err += chunk.toString('utf8'); });
    child.on('error',() => finish(new Error('computer-terminal-start')));
    // 'exit' fires before inherited pipes close: background children cannot keep this call open.
    child.on('exit',code => finish(null,code));
    signal?.addEventListener('abort',cancel,{once:true}); if (signal?.aborted) cancel();
  });
}
