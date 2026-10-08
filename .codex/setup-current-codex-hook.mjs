import {spawnSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {isDeepStrictEqual} from 'node:util';
const executable='C:/Users/Amir/AppData/Local/Roadeep/Roadeep.exe';
const relay='C:/Users/Amir/AppData/Local/com.roadeep.desktop/bin/roadeep-hook.exe';
const output='D:/Roadeep/Roadeep-mini/.codex/coding-hooks-review/hook-setup-result.json';
function run(args){
 const result=spawnSync(executable,args,{windowsHide:true,encoding:'utf8',maxBuffer:4*1024*1024,timeout:10000});
 if(result.error||result.status!==0)throw new Error('Native hook setup failed: '+(result.stderr?.trim()||result.error?.code||result.status));
 return JSON.parse(result.stdout);
}
const preview=run(['--coding-hooks-preview','codex']);
if(preview.provider!=='codex'||!preview.settingsPath.replaceAll('\\','/').endsWith('/.codex/hooks.json'))throw new Error('Unexpected hook destination');
const separator='\n+++ proposed\n';
const split=preview.diff.indexOf(separator);
if(!preview.diff.startsWith('--- current\n')||split<0)throw new Error('Invalid preview');
const before=JSON.parse(preview.diff.slice('--- current\n'.length,split));
const after=JSON.parse(preview.diff.slice(split+separator.length));
function stripOwned(value){
 const copy=structuredClone(value);
 for(const [event,entries] of Object.entries(copy.hooks??{})){
  if(!Array.isArray(entries))throw new Error('Invalid hook group');
  const command=`"${relay}" --provider codex ${event}`;
  const kept=[];
  for(const entry of entries){
   if(!Array.isArray(entry.hooks)){kept.push(entry);continue;}
   const length=entry.hooks.length;
   entry.hooks=entry.hooks.filter(handler=>handler.command!==command);
   if(length>0&&entry.hooks.length===0)continue;
   kept.push(entry);
  }
  if(kept.length)copy.hooks[event]=kept;else delete copy.hooks[event];
 }
 if(copy.hooks&&Object.keys(copy.hooks).length===0)delete copy.hooks;
 return copy;
}
if(!isDeepStrictEqual(stripOwned(before),stripOwned(after)))throw new Error('Preview would alter foreign configuration');
const events=[];
for(const [event,groups] of Object.entries(after.hooks??{})){
 const command=`"${relay}" --provider codex ${event}`;
 const owned=groups.flatMap(group=>group.hooks??[]).filter(handler=>handler.command===command);
 if(owned.length!==1)throw new Error('Expected one owned handler per event');
 events.push(event);
}
const result=run(['--coding-hooks-apply','codex',preview.fingerprint]);
if(result.provider!=='codex'||!result.installed||!result.hookReady)throw new Error('Installed hook verification failed');
const installed=JSON.parse(await readFile(preview.settingsPath,'utf8'));
if(!isDeepStrictEqual(installed,after))throw new Error('Installed configuration differs from reviewed preview');
const report={provider:'codex',installed:true,relayReady:true,scope:result.scope,events,
 destination:preview.settingsPath,backup:preview.backup,foreignConfigurationPreserved:true,
 upstreamTrust:'User must trust exact definitions through Codex /hooks; not bypassed',
 completedAt:new Date().toISOString()};
await writeFile(output,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report));
