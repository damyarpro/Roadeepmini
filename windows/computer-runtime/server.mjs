import net from 'node:net';
import fs from 'node:fs/promises';
import {chromium} from 'playwright';
import {fileOperation,terminal,validateOperation,publicUrl,INPUT_CAP} from './policy.mjs';
await fs.mkdir('/tmp/home',{recursive:true,mode:0o700});
// Network access is impossible in this container. Requests can only be fulfilled by the
// host's public-address pinned fetch transport; no browser websocket/service worker bypass.
const browser = await chromium.launch({headless:true,chromiumSandbox:false,args:['--disable-dev-shm-usage','--disable-background-networking','--disable-component-update','--disable-sync','--no-first-run']});
const context = await browser.newContext({viewport:{width:1024,height:640},serviceWorkers:'block',acceptDownloads:false});
context.setDefaultTimeout(10000); context.setDefaultNavigationTimeout(20000);
await context.routeWebSocket('**/*',socket => socket.close());
async function newPage() { const p = await context.newPage(); p.on('popup',popup => void popup.close()); p.on('dialog',dialog => void dialog.dismiss()); return p; }
let page = await newPage(); let active = null; let sequence = 0;
const pendingNetwork = new Map();
await context.route('**/*',async route => {
  const request = route.request(); const session = active;
  if (!session || session.signal.aborted || pendingNetwork.size >= 16) return route.abort('blockedbyclient');
  try {
    const url = publicUrl(request.url()); const body = request.postData();
    let previous = request.redirectedFrom(); let redirects = 0;
    while (previous) { redirects++; previous = previous.redirectedFrom(); }
    if (redirects > 3) throw new Error('computer-redirect-limit');
    if (body && Buffer.byteLength(body) > 128 * 1024) throw new Error('computer-network-limit');
    const id = ++sequence;
    const response = await new Promise((resolve,reject) => {
      const timer = setTimeout(() => { pendingNetwork.delete(id); reject(new Error('computer-network-timeout')); },14000);
      pendingNetwork.set(id,{resolve,reject,timer,session});
      session.socket.write(JSON.stringify({kind:'network',id,url,method:request.method(),headers:request.headers(),body})+'\n');
    });
    await route.fulfill({status:response.status,headers:response.headers,body:Buffer.from(response.body,'base64')});
  } catch { await route.abort('blockedbyclient').catch(() => {}); }
});
async function metadata() {
  const view = await page.evaluate(() => {
    const targets = [];
    for (const element of document.querySelectorAll('a,button,input,textarea,select,[role="button"],[role="link"]')) {
      const r = element.getBoundingClientRect();
      if (!r.width || !r.height || r.bottom < 0 || r.right < 0 || r.top >= 640 || r.left >= 1024 || getComputedStyle(element).visibility === 'hidden') continue;
      targets.push({role:element.getAttribute('role') || element.tagName.toLowerCase(),label:(element.getAttribute('aria-label') || element.innerText || element.getAttribute('placeholder') || element.getAttribute('name') || '').trim().slice(0,100),x:Math.min(1023,Math.max(0,Math.round(r.x+r.width/2))),y:Math.min(639,Math.max(0,Math.round(r.y+r.height/2)))});
      if (targets.length === 30) break;
    }
    return {visibleText:(document.body?.innerText || '').slice(0,6000),targets};
  });
  return {url:page.url().slice(0,4096),title:(await page.title()).slice(0,300),width:1024,height:640,...view};
}
async function operate(op,signal) {
  validateOperation(op);
  let data;
  switch (op.action) {
    case 'status': data = {state:'running',...await metadata()}; break;
    case 'navigate': await page.goto(publicUrl(op.url),{waitUntil:'domcontentloaded'}); data = await metadata(); break;
    case 'screenshot': {
      const bytes = await page.screenshot({type:'png',fullPage:false,timeout:10000});
      if (bytes.length > 1400000) throw new Error('computer-output-limit');
      return {kind:'result',ok:true,data:await metadata(),image:'data:image/png;base64,'+bytes.toString('base64')};
    }
    case 'click': await page.mouse.click(op.x,op.y); data = await metadata(); break;
    case 'type': await page.keyboard.insertText(op.text); data = await metadata(); break;
    case 'key': await page.keyboard.press(op.key); data = await metadata(); break;
    case 'scroll': await page.mouse.wheel(0,op.deltaY); data = await metadata(); break;
    case 'list': case 'read': case 'write': data = await fileOperation(op); break;
    case 'terminal': data = await terminal(op.command,{signal}); break;
  }
  return {kind:'result',ok:true,data};
}
const server = net.createServer(socket => {
  let buffer = ''; let started = false; let ctl;
  const close = () => {
    ctl?.abort();
    for (const [id,item] of pendingNetwork) if (item.session?.socket === socket) { clearTimeout(item.timer); item.reject(new Error('computer-cancelled')); pendingNetwork.delete(id); }
    if (active?.socket === socket) active = null;
  };
  socket.on('error',close); socket.on('close',close);
  socket.on('data',chunk => {
    buffer += chunk.toString('utf8'); if (Buffer.byteLength(buffer) > INPUT_CAP * 10) { socket.destroy(); return; }
    let end;
    while ((end = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0,end); buffer = buffer.slice(end+1); let value;
      try { value = JSON.parse(line); } catch { socket.destroy(); return; }
      if (started) {
        if (value.kind !== 'networkResult') { socket.destroy(); return; }
        const item = pendingNetwork.get(value.id); if (!item || item.session.socket !== socket) continue;
        clearTimeout(item.timer); pendingNetwork.delete(value.id); value.error ? item.reject(new Error('computer-network-refused')) : item.resolve(value.response); continue;
      }
      started = true;
      if (active) { socket.end(JSON.stringify({kind:'result',ok:false,error:'computer-busy'})+'\n'); return; }
      ctl = new AbortController(); const session = {socket,signal:ctl.signal}; active = session;
      const deadline = setTimeout(() => { ctl.abort(); socket.destroy(); void page.close().finally(async () => { page = await newPage(); }); },30000);
      operate(value,ctl.signal).then(result => { if (!socket.destroyed) socket.end(JSON.stringify(result)+'\n'); },() => { if (!socket.destroyed) socket.end(JSON.stringify({kind:'result',ok:false,error:'computer-operation-failed'})+'\n'); }).finally(() => { clearTimeout(deadline); if (active === session) active = null; });
    }
  });
});
await fs.rm('/tmp/roadeep-computer.sock',{force:true});
server.listen('/tmp/roadeep-computer.sock',async () => { await fs.chmod('/tmp/roadeep-computer.sock',0o600); });
process.on('SIGTERM',() => { server.close(); void browser.close().finally(() => process.exit(0)); });
// A host crash cannot leave an idle browser/container alive indefinitely.
let lastActivity = Date.now();
server.on('connection',() => { lastActivity = Date.now(); });
setInterval(() => { if (!active && Date.now()-lastActivity > 15*60*1000) { server.close(); void browser.close().finally(() => process.exit(0)); } },60000).unref();
