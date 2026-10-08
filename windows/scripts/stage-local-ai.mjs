// The native asset registry is the single source of build-time pins.
import {createHash,randomUUID} from "node:crypto";
import {createReadStream,createWriteStream} from "node:fs";
import {lstat,mkdir,readFile,readdir,rename,rm,writeFile} from "node:fs/promises";
import {dirname,join,resolve,relative,sep,isAbsolute} from "node:path";
import {fileURLToPath,pathToFileURL} from "node:url";
import {Transform} from "node:stream";
import {pipeline} from "node:stream/promises";

export const WINDOWS_ROOT=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const NATIVE_REGISTRY=join(WINDOWS_ROOT,"src-tauri/src/local_runtime/assets.rs");
const FILENAMES=["llama-vulkan.zip","llama-cpu.zip","whisper.zip","piper.zip","brain.gguf","whisper-model.bin","piper-model.onnx","piper-config.json","piper-MODEL_CARD"];
const METADATA=["manifest.json","THIRD-PARTY-NOTICES.md","README.md"];
const LICENSE_ROOT=join(WINDOWS_ROOT,"scripts/local-ai-licenses");
const LICENSES=[
 ["llama-MIT.txt",1078,"94f29bbed6a22c35b992c5c6ebf0e7c92f13b836b90f36f461c9cf2f0f1d010d","https://raw.githubusercontent.com/ggml-org/llama.cpp/b11388/LICENSE"],
 ["whisper-MIT.txt",1078,"94f29bbed6a22c35b992c5c6ebf0e7c92f13b836b90f36f461c9cf2f0f1d010d","https://raw.githubusercontent.com/ggml-org/whisper.cpp/b5130/LICENSE"],
 ["Piper-MIT.txt",1071,"4cd71dece7037f1d6d93cce7570c57ab75ea9ac566fd4990be2f3ab08d15b47f","https://raw.githubusercontent.com/rhasspy/piper/2023.11.14-2/LICENSE.md"],
 ["eSpeak-GPL-3.0.txt",35147,"8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903","https://raw.githubusercontent.com/rhasspy/espeak-ng/master/COPYING"],
 ["Qwen-Apache-2.0.txt",11544,"bbedc3fda3305820b977265f01b8619d87570a6739de3a5582c3464840f1e57a","https://huggingface.co/Qwen/Qwen3.5-2B/raw/main/LICENSE"],
 ["ONNXRuntime-MIT.txt",1073,"2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c","https://raw.githubusercontent.com/microsoft/onnxruntime/v1.14.1/LICENSE"],
 ["Piper-phonemize-MIT.txt",1071,"13746d509d74e55ea2265fbef204bb7cdbf84a8315b0207e988326cb54387028","https://raw.githubusercontent.com/rhasspy/piper-phonemize/master/LICENSE.md"],
 ["libtashkeel-MIT.txt",1066,"98dbc9a6a01badb391f2b64161742cb9cd8fcdc5cfc97486be0d49c47e98d1a8","https://raw.githubusercontent.com/mush42/libtashkeel/main/LICENSE"],
 ["Sherpa-ONNX-Apache-2.0.txt",11358,"cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30","https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/LICENSE"],
 ["CAMplusplus-Apache-2.0.txt",11357,"c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4","https://raw.githubusercontent.com/modelscope/3D-Speaker/main/LICENSE"],
].map(([file,size,sha256,url])=>({file,size,sha256,url}));

export function parsePins(source){
 const assets=[];
 for(const match of source.split("#[cfg(test)]")[0].matchAll(/Asset\s*\{([^{}]+)\}/g)){
  const body=match[1];const string=key=>body.match(new RegExp(`\\b${key}\\s*:\\s*"([^"\\n]+)"`))?.[1];
  const id=string("id");if(!id)continue;
  assets.push({id,file:string("cache"),url:string("url"),size:Number(body.match(/\bsize\s*:\s*(\d[\d_]*)/)?.[1].replaceAll("_","")),sha256:string("sha256"),destination:string("destination"),archive:/\barchive\s*:\s*true/.test(body)});
 }
 if(FILENAMES.some(file=>!assets.some(asset=>asset.file===file)))throw new Error("Native local AI registry does not match the offline bundle contract");
 validatePins(assets);return assets;
}
function validatePins(assets){
 const names=new Set();
 for(const asset of assets){
  if(!/^[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(asset.file)||names.has(asset.file)||!Number.isSafeInteger(asset.size)||asset.size<=0||asset.size>2*1024**3||!/^[a-f0-9]{64}$/.test(asset.sha256)||!/^https:\/\//.test(asset.url))throw new Error("Invalid fixed asset pin");
  names.add(asset.file);
 }
}
export async function loadPins(){return parsePins(await readFile(NATIVE_REGISTRY,"utf8"));}
async function regularFile(path){const info=await lstat(path);if(!info.isFile()||info.isSymbolicLink())throw new Error("Bundle source must be a regular file");return info;}
async function directory(path){const info=await lstat(path);if(!info.isDirectory()||info.isSymbolicLink())throw new Error("Bundle directory must not be a symbolic link");}
async function commitRename(source,destination){
 // Windows scanners can briefly hold freshly copied model files without delete sharing.
 for(let attempt=0;;attempt++){
  try{await rename(source,destination);return;}catch(error){
   if(attempt>=20||!["EPERM","EBUSY","EACCES"].includes(error.code))throw error;
   await new Promise(resolve=>setTimeout(resolve,250));
  }
 }
}
async function copyVerified(source,destination,pin){
 const info=await regularFile(source);if(info.size!==pin.size)throw new Error(`Asset size mismatch: ${pin.file}`);
 let size=0;const hash=createHash("sha256");
 const verifier=new Transform({transform(chunk,_encoding,callback){size+=chunk.length;if(size>pin.size){callback(new Error(`Asset exceeds pinned size: ${pin.file}`));return;}hash.update(chunk);callback(null,chunk);}});
 await pipeline(createReadStream(source,{highWaterMark:256*1024}),verifier,createWriteStream(destination,{flags:"wx",highWaterMark:256*1024}));
 if(size!==pin.size||hash.digest("hex")!==pin.sha256)throw new Error(`Asset integrity mismatch: ${pin.file}`);
}
async function checkFile(path,pin){
 const info=await regularFile(path);if(info.size!==pin.size)throw new Error(`Asset size mismatch: ${pin.file}`);
 const hash=createHash("sha256");let size=0;
 for await(const chunk of createReadStream(path,{highWaterMark:256*1024})){size+=chunk.length;if(size>pin.size)throw new Error(`Asset exceeds pinned size: ${pin.file}`);hash.update(chunk);}
 if(size!==pin.size||hash.digest("hex")!==pin.sha256)throw new Error(`Asset integrity mismatch: ${pin.file}`);
}
function manifest(assets){return{version:1,totalBytes:assets.reduce((sum,asset)=>sum+asset.size,0),assets,licenses:LICENSES};}
const NOTICES=`# Local AI third-party notices

The offline installer preserves original upstream archives and the Persian voice
MODEL_CARD alongside immutable URLs, sizes and SHA-256 hashes in manifest.json.
No API key or cloud-provider model is included. The files below are prepared
locally on first launch; the package does not fetch model files from the network.

- llama.cpp b11388: MIT; https://github.com/ggml-org/llama.cpp/tree/b11388
  The original archives also preserve the LLVM/OpenMP license notice.
- Qwen3.5-2B: Apache-2.0. Quantized GGUF source:
  https://huggingface.co/unsloth/Qwen3.5-2B-GGUF
- whisper.cpp b5130 and Whisper weights: MIT.
  https://github.com/ggml-org/whisper.cpp/tree/b5130
  https://github.com/openai/whisper
- Piper 2023.11.14-2: MIT.
  https://github.com/rhasspy/piper/tree/2023.11.14-2
- Piper includes eSpeak NG, distributed under GPL-3.0.
  Source: https://github.com/rhasspy/espeak-ng
  License: https://github.com/espeak-ng/espeak-ng/blob/master/COPYING
  Preserve its license and source availability when redistributing these binaries.
- Persian Amir voice dataset: CC0, per the original piper-MODEL_CARD included
  in this directory. The exact voice and config revisions appear in manifest.json.
  https://huggingface.co/rhasspy/piper-voices
- Sherpa-ONNX v1.13.8 speaker extraction runtime: Apache-2.0.
  https://github.com/k2-fsa/sherpa-onnx/tree/v1.13.8
- 3D-Speaker CAM++ speaker embedding model: Apache-2.0.
  https://github.com/modelscope/3D-Speaker

Full license texts are preserved in licenses/, with source URLs and checksums
in manifest.json. Piper also includes ONNX Runtime, piper-phonemize and
libtashkeel under MIT; their copyright notices are included there. Sherpa-ONNX
also uses ONNX Runtime; the original runtime archive is shipped unchanged.
See the upstream source repositories and original license notices for full terms.
The package metadata does not modify or replace those terms.
`;
const README=`# Offline local AI packages

Generated by windows/scripts/stage-local-ai.mjs from the native pinned registry.
All files in the compiled asset registry are shipped unchanged, with exact sizes and SHA-256 in
manifest.json. Weights and archives are generated build artifacts and are not
tracked by git. Tauri maps this directory to resource_dir/local-ai-packages.

First launch prepares verified app-owned runtimes locally. A present but corrupt
or incomplete offline bundle fails visibly instead of silently downloading data.
Existing verified per-user runtimes are reused. No separate AI app is required.

From windows/: node scripts/stage-local-ai.mjs --source <trusted-asset-directory>
Then: npm run pack. ROADEEP_LOCAL_ASSET_SOURCE can stage an explicit trusted
source before pack. Packaging otherwise verifies an already staged bundle.
No network access is performed by the staging script.
`;

// Exported destination/pins are used by isolated tests, not exposed as CLI options.
export async function stageBundle({source,destination=join(WINDOWS_ROOT,"local-ai-bundle"),assets,onProgress=()=>{}}){
 assets??=await loadPins();validatePins(assets);source=resolve(source);destination=resolve(destination);
 await directory(source);await directory(dirname(destination));
 const contains=(base,target)=>{const path=relative(base,target);return path===""||(!path.startsWith(`..${sep}`)&&path!==".."&&!isAbsolute(path));};
 if(contains(source,destination)||contains(destination,source))throw new Error("Source and destination must be separate directories");
 const allowed=new Set([...assets.map(asset=>asset.file),...METADATA,"licenses"]);
 try{await directory(destination);for(const file of await readdir(destination))if(!allowed.has(file))throw new Error(`Unknown bundle destination file: ${file}`);}catch(error){if(error.code!=="ENOENT")throw error;}
 try{await directory(join(destination,"licenses"));for(const file of await readdir(join(destination,"licenses")))if(!LICENSES.some(license=>license.file===file))throw new Error(`Unknown bundle license file: ${file}`);}catch(error){if(error.code!=="ENOENT")throw error;}
 const stage=join(dirname(destination),`.local-ai-bundle-stage-${randomUUID()}`);const backup=join(dirname(destination),`.local-ai-bundle-previous-${randomUUID()}`);
 await mkdir(stage);let previous=false;let committed=false;
 try{
  for(const asset of assets){await copyVerified(join(source,asset.file),join(stage,asset.file),asset);onProgress(asset.file);}
  await mkdir(join(stage,"licenses"));
  for(const license of LICENSES)await copyVerified(join(LICENSE_ROOT,license.file),join(stage,"licenses",license.file),license);
  const metadata=manifest(assets);
  await writeFile(join(stage,"manifest.json"),JSON.stringify(metadata,null,2)+"\n");await writeFile(join(stage,"THIRD-PARTY-NOTICES.md"),NOTICES);await writeFile(join(stage,"README.md"),README);
  try{await commitRename(destination,backup);previous=true;}catch(error){if(error.code!=="ENOENT")throw error;}
  try{await commitRename(stage,destination);committed=true;}catch(error){if(previous)await commitRename(backup,destination);throw error;}
  if(previous)await rm(backup,{recursive:true});return metadata;
 }finally{if(!committed)await rm(stage,{recursive:true,force:true});}
}
export async function verifyBundle(destination=join(WINDOWS_ROOT,"local-ai-bundle"),assets){
 assets??=await loadPins();validatePins(assets);await directory(destination);
 const data=await readFile(join(destination,"manifest.json"),"utf8");if(data.length>65536)throw new Error("Invalid offline bundle manifest");
 if(JSON.stringify(JSON.parse(data))!==JSON.stringify(manifest(assets)))throw new Error("Offline bundle manifest does not match native pins");
 for(const asset of assets)await checkFile(join(destination,asset.file),asset);
 for(const file of METADATA)await regularFile(join(destination,file));
 await directory(join(destination,"licenses"));
 for(const license of LICENSES)await checkFile(join(destination,"licenses",license.file),license);
 return manifest(assets);
}
export async function prepareBundle(){
 if(process.env.ROADEEP_LOCAL_ASSET_SOURCE)await stageBundle({source:process.env.ROADEEP_LOCAL_ASSET_SOURCE,onProgress:file=>console.log(`  Local AI package verified: ${file}`)});
 return verifyBundle();
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 const arguments_=process.argv.slice(2);const source=arguments_[0]==="--source"&&arguments_.length===2?arguments_[1]:undefined;
 try{if(!source)throw new Error("Usage: node scripts/stage-local-ai.mjs --source <trusted-asset-directory>");const result=await stageBundle({source,onProgress:file=>console.log(`Verified ${file}`)});console.log(`Offline local AI package ready: ${result.totalBytes} bytes in ${result.assets.length} assets`);}catch(error){console.error(error.message);process.exitCode=1;}
}
