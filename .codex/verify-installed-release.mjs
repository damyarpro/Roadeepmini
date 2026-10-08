import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {readFile,stat,writeFile} from 'node:fs/promises';
import {resolve,relative,isAbsolute} from 'node:path';
const workspace='D:/Roadeep/Roadeep-mini';
const config=JSON.parse(await readFile(resolve(workspace,'windows/src-tauri/tauri.conf.json'),'utf8'));
const version=config.version;
const installed='C:/Users/Amir/AppData/Local/Roadeep/Roadeep.exe';
const build=resolve(workspace,'windows/target/release/Roadeep.exe');
const runtime='C:/Users/Amir/AppData/Local/com.roadeep.desktop/local-ai/v1';
async function hashFile(path){const hash=createHash('sha256');for await(const data of createReadStream(path))hash.update(data);return hash.digest('hex');}
const source=await readFile(build),target=await readFile(installed);
const marker=Buffer.from('__TAURI_BUNDLE_TYPE_VAR_UNK');
const offset=source.indexOf(marker);
if(offset<0||source.indexOf(marker,offset+1)>=0)throw new Error('Unexpected bundle marker');
Buffer.from('NSS').copy(source,offset+marker.length-3);
const expected=createHash('sha256').update(source).digest('hex');
const actual=createHash('sha256').update(target).digest('hex');
if(expected!==actual)throw new Error('Installed binary does not match NSIS release');
const receipt=JSON.parse(await readFile(resolve(runtime,'receipt.json'),'utf8'));
let verified=0;
for(const file of receipt.files){
 const path=resolve(runtime,file.path),child=relative(runtime,path);
 if(child.startsWith('..')||isAbsolute(child))throw new Error('Receipt path outside runtime');
 if((await stat(path)).size!==file.size||await hashFile(path)!==file.sha256)throw new Error('Runtime receipt mismatch');
 verified++;
}
const installer=resolve(workspace,`windows/release/Roadeep-Windows-${version}-setup.exe`);
const installerHash=await hashFile(installer);
await writeFile(installer+'.sha256',installerHash+`  Roadeep-Windows-${version}-setup.exe\n`);
const report={version,installedBinaryMatches:true,installedSha256:actual,runtimeFilesVerified:verified,installerBytes:(await stat(installer)).size,installerSha256:installerHash};
await writeFile(resolve(workspace,'.codex/voice-meter-review/release-verification.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report));
