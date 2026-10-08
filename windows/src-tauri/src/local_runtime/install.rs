use super::{assets, RuntimePaths, Status};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MAX_EXPANDED: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 2048;
#[derive(Serialize, Deserialize)]
struct Receipt {
    version: u32,
    files: Vec<FileHash>,
}
#[derive(Serialize, Deserialize)]
struct FileHash {
    path: String,
    size: u64,
    sha256: String,
}
fn link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    let attributes = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
    };
    #[cfg(not(windows))]
    let attributes = 0;
    unsafe_link(metadata.file_type().is_symlink(), attributes)
}
fn unsafe_link(symlink: bool, attributes: u32) -> bool {
    symlink || attributes & 0x400 != 0
}
fn no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if link(&metadata) => return Err("Unsafe runtime storage link".into()),
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err("Cannot inspect runtime storage".into()),
        }
    }
    Ok(())
}
pub(crate) fn safe_directory(path: &Path) -> Result<(), String> {
    no_links(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() => return Err("Unsafe runtime storage directory".into()),
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|_| "Cannot create runtime storage")?
        }
        Err(_) => return Err("Cannot inspect runtime storage".into()),
    }
    no_links(path)
}
fn relative(value: &str) -> Result<PathBuf, String> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || value.split('/').any(|p| p.ends_with(['.', ' ']))
    {
        return Err("Unsafe runtime archive path".into());
    }
    let p = PathBuf::from(value);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("Unsafe runtime archive path".into());
    }
    Ok(p)
}
fn digest(path: &Path) -> Result<(u64, String), String> {
    no_links(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| "Runtime asset missing")?;
    if !meta.is_file() || link(&meta) {
        return Err("Unsafe runtime asset".into());
    }
    let mut f = fs::File::open(path).map_err(|_| "Cannot read runtime asset")?;
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = f.read(&mut b).map_err(|_| "Cannot verify runtime asset")?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok((meta.len(), format!("{:x}", h.finalize())))
}
fn locate(root: &Path, name: &str) -> Result<PathBuf, String> {
    let mut found = Vec::new();
    fn visit(p: &Path, name: &str, found: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("Runtime directory nesting exceeds limit".into());
        }
        for e in fs::read_dir(p).map_err(|_| "Cannot inspect runtime directory")? {
            let e = e.map_err(|_| "Cannot inspect runtime directory")?;
            let metadata =
                fs::symlink_metadata(e.path()).map_err(|_| "Cannot inspect runtime file")?;
            let t = metadata.file_type();
            if link(&metadata) {
                return Err("Unsafe runtime symbolic link".into());
            }
            if t.is_dir() {
                visit(&e.path(), name, found, depth + 1)?;
            } else if e.file_name().to_string_lossy() == name {
                found.push(e.path());
            }
        }
        Ok(())
    }
    no_links(root)?;
    visit(root, name, &mut found, 0)?;
    if found.len() != 1 {
        return Err(format!("Runtime executable {name} is missing or ambiguous"));
    }
    Ok(found.remove(0))
}
fn runtime_paths(root: &Path) -> Result<RuntimePaths, String> {
    Ok(RuntimePaths {
        root: root.into(),
        llama_exe: locate(&root.join("llama"), "llama-cli.exe")?,
        llama_cpu_exe: locate(&root.join("llama-cpu"), "llama-cli.exe")?,
        brain_model: root.join("models/brain.gguf"),
        whisper_exe: locate(&root.join("whisper"), "whisper-cli.exe")?,
        whisper_model: root.join("models/whisper.bin"),
        piper_exe: locate(&root.join("piper"), "piper.exe")?,
        piper_model: root.join("models/fa.onnx"),
        piper_config: root.join("models/fa.onnx.json"),
        piper_espeak_data: root.join("piper/piper/espeak-ng-data"),
        speaker_dll: root.join("speaker/sherpa-onnx-c-api.dll"),
        speaker_model: root.join("models/speaker.onnx"),
    })
}
pub(crate) fn verify(root: &Path) -> Result<Option<RuntimePaths>, String> {
    if !root.exists() {
        return Ok(None);
    }
    safe_directory(root)?;
    let receipt_path = root.join("receipt.json");
    if !receipt_path.exists() {
        return Ok(None);
    }
    no_links(&receipt_path)?;
    if fs::metadata(&receipt_path)
        .map_err(|_| "Runtime receipt unavailable")?
        .len()
        > 512 * 1024
    {
        return Err("Invalid runtime receipt".into());
    }
    let bytes = fs::read(&receipt_path).map_err(|_| "Runtime verification receipt unavailable")?;
    if bytes.len() > 512 * 1024 {
        return Err("Invalid runtime receipt".into());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| "Invalid runtime receipt")?;
    if receipt.version != 1 || receipt.files.is_empty() || receipt.files.len() > MAX_ENTRIES {
        return Err("Invalid runtime receipt".into());
    }
    for file in &receipt.files {
        let p = root.join(relative(&file.path)?);
        let (size, hash) = digest(&p)?;
        if size != file.size || hash != file.sha256 {
            return Err("Runtime integrity check failed; reinstall required".into());
        }
    }
    // Model hashes stay anchored to source metadata, not only the writable receipt.
    for asset in assets::all() {
        let asset_path = if asset.archive {
            root.join(format!("packages/{}.zip", asset.id))
        } else {
            root.join(asset.destination)
        };
        if asset.id.starts_with("speaker-") && !asset_path.exists() {
            return Ok(None);
        }
        let (size, hash) = digest(&asset_path)?;
        if size != asset.size || hash != asset.sha256 {
            return Err("Pinned asset integrity check failed".into());
        }
        if asset.archive {
            verify_archive(&asset_path, &root.join(asset.destination))?;
        }
    }
    Ok(Some(runtime_paths(root)?))
}
pub(crate) fn read_enabled(root: &Path) -> Result<bool, String> {
    let config = root.join("enabled.json");
    no_links(&config)?;
    if let Ok(metadata) = fs::metadata(&config) {
        if metadata.len() > 5 {
            return Err("Invalid local runtime configuration".into());
        }
    }
    match fs::read(config) {
        Ok(b) => serde_json::from_slice::<bool>(&b)
            .map_err(|_| "Invalid local runtime configuration".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Cannot read local runtime configuration".into()),
    }
}
pub(crate) fn write_enabled(root: &Path, enabled: bool) -> Result<(), String> {
    safe_directory(root)?;
    let pending = root.join(format!("enabled-{}.pending", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "Cannot save local runtime configuration")?;
    if let Err(error) = file
        .write_all(if enabled { b"true" } else { b"false" })
        .and_then(|_| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&pending);
        return Err(format!(
            "Cannot save local runtime configuration: {}",
            error.kind()
        ));
    }
    drop(file);
    if fs::rename(&pending, root.join("enabled.json")).is_err() {
        let _ = fs::remove_file(pending);
        return Err("Cannot commit local runtime configuration".into());
    }
    Ok(())
}
fn check(cancel: &AtomicU64, generation: u64) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) != generation {
        Err("Installation cancelled".into())
    } else {
        Ok(())
    }
}
fn extract(
    archive: &Path,
    destination: &Path,
    cancel: &AtomicU64,
    generation: u64,
) -> Result<(), String> {
    safe_directory(destination)?;
    if bz2(archive)? {
        return extract_speaker_tar(archive, destination, cancel, generation);
    }
    let f = fs::File::open(archive).map_err(|_| "Runtime archive unavailable")?;
    let mut zip = zip::ZipArchive::new(f).map_err(|_| "Invalid runtime archive")?;
    if zip.len() > MAX_ENTRIES {
        return Err("Runtime archive has too many entries".into());
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        check(cancel, generation)?;
        let mut entry = zip
            .by_index(i)
            .map_err(|_| "Invalid runtime archive entry")?;
        if entry
            .unix_mode()
            .map(|m| m & 0o170000 == 0o120000)
            .unwrap_or(false)
        {
            return Err("Runtime archive contains symbolic link".into());
        }
        let name = entry.name().trim_end_matches('/');
        let target = destination.join(relative(name)?);
        total = total
            .checked_add(entry.size())
            .ok_or("Runtime archive size overflow")?;
        if total > MAX_EXPANDED {
            return Err("Runtime archive exceeds extraction limit".into());
        }
        if entry.is_dir() {
            safe_directory(&target)?;
            continue;
        }
        safe_directory(target.parent().ok_or("Invalid archive path")?)?;
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|_| "Duplicate or inaccessible runtime archive file")?;
        let expected = entry.size();
        let copied = std::io::copy(&mut entry.by_ref().take(expected + 1), &mut out)
            .map_err(|_| "Runtime archive extraction failed")?;
        if copied != entry.size() {
            return Err("Runtime archive entry size mismatch".into());
        }
    }
    Ok(())
}
fn verify_archive(archive: &Path, destination: &Path) -> Result<(), String> {
    if bz2(archive)? {
        return verify_speaker_tar(archive, destination);
    }
    let file = fs::File::open(archive).map_err(|_| "Runtime archive unavailable")?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "Invalid runtime archive")?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| "Invalid runtime archive")?;
        if entry.is_dir() {
            continue;
        }
        let p = destination.join(relative(entry.name())?);
        let (size, actual) = digest(&p)?;
        if size != entry.size() {
            return Err("Executable integrity mismatch".into());
        }
        let mut hash = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|_| "Runtime archive integrity failure")?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        if actual != format!("{:x}", hash.finalize()) {
            return Err("Executable integrity mismatch".into());
        }
    }
    Ok(())
}
const SPEAKER_DLLS: [&str; 3] = [
    "sherpa-onnx-c-api.dll",
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
];
fn bz2(path: &Path) -> Result<bool, String> {
    let mut file = fs::File::open(path).map_err(|_| "Runtime archive unavailable")?;
    let mut magic = [0; 3];
    file.read_exact(&mut magic)
        .map_err(|_| "Invalid runtime archive")?;
    Ok(&magic == b"BZh")
}
fn speaker_tar(path: &Path) -> Result<tar::Archive<bzip2::read::BzDecoder<fs::File>>, String> {
    Ok(tar::Archive::new(bzip2::read::BzDecoder::new(
        fs::File::open(path).map_err(|_| "Runtime archive unavailable")?,
    )))
}
fn speaker_entry(path: &Path) -> Result<Option<String>, String> {
    let value = path
        .to_str()
        .ok_or("Unsafe runtime archive path")?
        .trim_end_matches('/');
    let safe = relative(value)?;
    let name = safe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Unsafe runtime archive path")?;
    Ok(
        if SPEAKER_DLLS.contains(&name)
            && safe
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|p| p.to_str())
                == Some("lib")
        {
            Some(name.into())
        } else {
            None
        },
    )
}
fn extract_speaker_tar(
    archive: &Path,
    destination: &Path,
    cancel: &AtomicU64,
    generation: u64,
) -> Result<(), String> {
    let mut archive = speaker_tar(archive)?;
    let mut total = 0u64;
    let mut entries = 0;
    let mut found = std::collections::HashSet::new();
    for item in archive.entries().map_err(|_| "Invalid speaker archive")? {
        check(cancel, generation)?;
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err("Runtime archive has too many entries".into());
        }
        let mut entry = item.map_err(|_| "Invalid speaker archive entry")?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("Unsafe speaker archive entry type".into());
        }
        let chosen = speaker_entry(&entry.path().map_err(|_| "Invalid speaker archive path")?)?;
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or("Runtime archive size overflow")?;
        if total > MAX_EXPANDED {
            return Err("Runtime archive exceeds extraction limit".into());
        }
        if let Some(name) = chosen {
            if !kind.is_file() || !found.insert(name.clone()) {
                return Err("Duplicate speaker runtime DLL".into());
            }
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination.join(name))
                .map_err(|_| "Cannot create speaker runtime DLL")?;
            let copied = std::io::copy(&mut entry.by_ref().take(size + 1), &mut file)
                .map_err(|_| "Speaker runtime extraction failed")?;
            if copied != size {
                return Err("Speaker runtime archive size mismatch".into());
            }
        }
    }
    if found.len() != SPEAKER_DLLS.len() {
        return Err("Speaker runtime DLLs missing".into());
    }
    Ok(())
}
fn verify_speaker_tar(archive: &Path, destination: &Path) -> Result<(), String> {
    let mut archive = speaker_tar(archive)?;
    let mut total = 0u64;
    let mut count = 0;
    let mut found = std::collections::HashSet::new();
    for item in archive.entries().map_err(|_| "Invalid speaker archive")? {
        count += 1;
        if count > MAX_ENTRIES {
            return Err("Runtime archive has too many entries".into());
        }
        let mut entry = item.map_err(|_| "Invalid speaker archive entry")?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("Unsafe speaker archive entry type".into());
        }
        let chosen = speaker_entry(&entry.path().map_err(|_| "Invalid speaker archive path")?)?;
        total = total
            .checked_add(entry.size())
            .ok_or("Runtime archive size overflow")?;
        if total > MAX_EXPANDED {
            return Err("Runtime archive exceeds extraction limit".into());
        }
        if let Some(name) = chosen {
            if !kind.is_file() || !found.insert(name.clone()) {
                return Err("Duplicate speaker runtime DLL".into());
            }
            let (size, expected) = digest(&destination.join(name))?;
            if size != entry.size() {
                return Err("Speaker DLL integrity mismatch".into());
            }
            let mut hash = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let n = entry
                    .read(&mut buffer)
                    .map_err(|_| "Speaker archive integrity failure")?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            if expected != format!("{:x}", hash.finalize()) {
                return Err("Speaker DLL integrity mismatch".into());
            }
        }
    }
    if found.len() != 3 {
        return Err("Speaker runtime DLLs missing".into());
    }
    Ok(())
}
fn receipt(root: &Path) -> Result<Receipt, String> {
    fn collect(root: &Path, p: &Path, files: &mut Vec<FileHash>) -> Result<(), String> {
        for entry in fs::read_dir(p).map_err(|_| "Runtime receipt failed")? {
            let e = entry.map_err(|_| "Runtime receipt failed")?;
            let metadata = fs::symlink_metadata(e.path()).map_err(|_| "Runtime receipt failed")?;
            if link(&metadata) {
                return Err("Unsafe runtime storage link".into());
            }
            let t = metadata.file_type();
            if t.is_dir() {
                collect(root, &e.path(), files)?;
            } else {
                let (size, sha256) = digest(&e.path())?;
                files.push(FileHash {
                    path: e
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| "Invalid runtime path")?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    size,
                    sha256,
                });
            }
        }
        Ok(())
    }
    let mut files = vec![];
    collect(root, root, &mut files)?;
    if files.len() > MAX_ENTRIES {
        return Err("Too many runtime files".into());
    }
    Ok(Receipt { version: 1, files })
}
/// The presence of a bundled directory selects an exclusively offline source.
fn validate_bundle(directory: &Path) -> Result<(), String> {
    no_links(directory)?;
    if !fs::metadata(directory)
        .map_err(|_| "Bundled runtime directory missing")?
        .is_dir()
    {
        return Err("Invalid bundled runtime directory".into());
    }
    for asset in assets::all() {
        let source = directory.join(asset.cache);
        no_links(&source)?;
        let metadata = fs::symlink_metadata(source)
            .map_err(|_| "Bundled runtime asset missing; reinstall the application")?;
        if !metadata.is_file() || link(&metadata) || metadata.len() != asset.size {
            return Err("Bundled runtime asset is incomplete; reinstall the application".into());
        }
    }
    Ok(())
}
fn asset_source(
    bundle: Option<&Path>,
    cache: &Path,
    asset: &assets::Asset,
) -> Result<Option<PathBuf>, String> {
    if let Some(directory) = bundle {
        let source = directory.join(asset.cache);
        no_links(&source)?;
        let metadata = fs::symlink_metadata(&source)
            .map_err(|_| "Bundled runtime asset missing; network fallback is disabled")?;
        if !metadata.is_file() || link(&metadata) || metadata.len() != asset.size {
            return Err("Bundled runtime asset is incomplete; network fallback is disabled".into());
        }
        return Ok(Some(source));
    }
    Ok(if cache.exists() {
        Some(cache.into())
    } else {
        None
    })
}
#[cfg(test)]
pub async fn install_at(
    root: &Path,
    cancel: Arc<AtomicU64>,
    generation: u64,
    progress: impl Fn(Status),
) -> Result<RuntimePaths, String> {
    install_from(root, cancel, generation, progress, None).await
}
pub(crate) async fn install_from(
    root: &Path,
    cancel: Arc<AtomicU64>,
    generation: u64,
    progress: impl Fn(Status),
    bundled: Option<&Path>,
) -> Result<RuntimePaths, String> {
    let enabled = if root.exists() {
        read_enabled(root)?
    } else {
        true
    };
    if let Some(directory) = bundled {
        validate_bundle(directory)?;
    }
    let parent = root.parent().ok_or("Invalid runtime root")?;
    let stage = parent.join(format!("staging-{}", uuid::Uuid::new_v4()));
    safe_directory(&stage)?;
    let result=async {
        let client=if bundled.is_none(){Some(reqwest::Client::builder().timeout(Duration::from_secs(3600)).connect_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(30)).redirect(reqwest::redirect::Policy::limited(5)).build().map_err(|_|"Runtime download client unavailable")?)}else{None};
        let mut downloaded=0u64;let mut emitted=Instant::now();
        for asset in assets::all(){check(&cancel,generation)?;let temp=stage.join("download.part");
            let cached=parent.join("downloads").join(asset.cache);
            let source_path=asset_source(bundled,&cached,asset)?;
            let mut bytes=0u64;
            if let Some(cache)=source_path{
                no_links(&cache)?;
                let mut source=fs::File::open(&cache).map_err(|_|"Cannot open cached runtime asset")?;
                let mut dest=fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|_|"Cannot stage cached runtime asset")?;
                let mut hash=Sha256::new();let mut buffer=[0u8;65536];
                loop{check(&cancel,generation)?;let n=source.read(&mut buffer).map_err(|_|"Cannot read cached runtime asset")?;if n==0{break;}bytes+=n as u64;if bytes>asset.size{return Err("Cached asset exceeds pinned size".into());}hash.update(&buffer[..n]);dest.write_all(&buffer[..n]).map_err(|_|"Cannot stage cached runtime asset")?;
                    if emitted.elapsed()>=Duration::from_millis(250){progress(Status{phase:"verifying".into(),component:asset.id.into(),downloaded:downloaded+bytes,..Status::default()});emitted=Instant::now();}}
                dest.sync_all().map_err(|_|"Runtime storage sync failed")?;
                if bytes!=asset.size||format!("{:x}",hash.finalize())!=asset.sha256{return Err("Cached runtime asset integrity failed".into());}
            }else{
            let sending=client.as_ref().ok_or("Bundled runtime asset missing; network fallback is disabled")?.get(asset.url).send();tokio::pin!(sending);
            let cancellation=async {loop{if cancel.load(Ordering::Acquire)!=generation{break;}tokio::time::sleep(Duration::from_millis(250)).await;}};
            let mut response=tokio::select!{result=&mut sending=>result.map_err(|_|"Runtime download connection failed")?,_=cancellation=>return Err("Installation cancelled".into())};
            if !response.status().is_success(){return Err(format!("Runtime download failed (HTTP {})",response.status().as_u16()));}
            if response.url().scheme()!="https"{return Err("Insecure runtime download redirect".into());}
            if response.content_length().is_some_and(|n|n!=asset.size){return Err("Runtime download length mismatch".into());}
            let mut f=fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|_|"Cannot create runtime download")?;let mut hash=Sha256::new();
            loop{check(&cancel,generation)?;
                let chunk=tokio::select!{c=response.chunk()=>c.map_err(|_|"Runtime download interrupted")?,_=tokio::time::sleep(Duration::from_millis(250))=>continue};
                let Some(chunk)=chunk else{break};bytes+=chunk.len() as u64;if bytes>asset.size{return Err("Runtime download exceeds pinned size".into());}hash.update(&chunk);f.write_all(&chunk).map_err(|_|"Runtime storage write failed")?;
                if emitted.elapsed()>=Duration::from_millis(250){progress(Status{phase:"downloading".into(),component:asset.id.into(),downloaded:downloaded+bytes,..Status::default()});emitted=Instant::now();}
            }
            f.sync_all().map_err(|_|"Runtime storage sync failed")?;drop(f);
            if bytes!=asset.size||format!("{:x}",hash.finalize())!=asset.sha256{return Err("Runtime download integrity check failed".into());}
            }
            check(&cancel,generation)?;downloaded+=bytes;
            progress(Status{phase:"installing".into(),component:asset.id.into(),downloaded,..Status::default()});
            if asset.archive{let archive=temp.clone();let destination=stage.join(asset.destination);let cancelled=cancel.clone();tauri::async_runtime::spawn_blocking(move||extract(&archive,&destination,&cancelled,generation)).await.map_err(|_|"Runtime extraction task failed")??;safe_directory(&stage.join("packages"))?;fs::rename(&temp,stage.join(format!("packages/{}.zip",asset.id))).map_err(|_|"Cannot retain verified archive")?;}
            else{let dest=stage.join(asset.destination);safe_directory(dest.parent().ok_or("Invalid asset path")?)?;fs::rename(&temp,dest).map_err(|_|"Cannot install verified asset")?;}
        }
        let p=stage.clone();let record=tauri::async_runtime::spawn_blocking(move||receipt(&p)).await.map_err(|_|"Runtime receipt task failed")??;
        runtime_paths(&stage)?;fs::write(stage.join("receipt.json"),serde_json::to_vec(&record).map_err(|_|"Runtime receipt serialization failed")?).map_err(|_|"Cannot write runtime receipt")?;
        write_enabled(&stage,enabled)?;check(&cancel,generation)?;
        let backup=parent.join(format!("previous-{}",uuid::Uuid::new_v4()));let had=root.exists();if had{safe_directory(root)?;fs::rename(root,&backup).map_err(|_|"Cannot replace previous local runtime")?;}
        if fs::rename(&stage,root).is_err(){if had{let _=fs::rename(&backup,root);}return Err("Cannot commit local runtime installation".into());}
        if had{if let Err(e)=fs::remove_dir_all(&backup){crate::log::line(format!("local runtime previous version cleanup failed: {}",e.kind()));}}
        runtime_paths(root)
    }.await;
    if stage.exists() {
        if let Err(e) = fs::remove_dir_all(&stage) {
            crate::log::line(format!(
                "local runtime staging cleanup failed: {}",
                e.kind()
            ));
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_source_never_falls_back_to_existing_cache() {
        let directory =
            std::env::temp_dir().join(format!("roadeep-offline-source-{}", uuid::Uuid::new_v4()));
        safe_directory(&directory).unwrap();
        let cache = directory.join("cached.bin");
        fs::write(&cache, b"valid").unwrap();
        let asset = assets::Asset {
            id: "test",
            url: "https://example.invalid",
            size: 5,
            sha256: "unused",
            destination: "unused",
            archive: false,
            cache: "bundle.bin",
        };
        assert_eq!(
            asset_source(None, &cache, &asset).unwrap(),
            Some(cache.clone())
        );
        assert!(asset_source(Some(&directory), &cache, &asset).is_err());
        fs::write(directory.join("bundle.bin"), b"bad").unwrap();
        assert!(asset_source(Some(&directory), &cache, &asset).is_err());
        fs::write(directory.join("bundle.bin"), b"valid").unwrap();
        assert_eq!(
            asset_source(Some(&directory), &cache, &asset).unwrap(),
            Some(directory.join("bundle.bin"))
        );
        fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    async fn incomplete_bundle_fails_before_install_or_network() {
        let directory = std::env::temp_dir().join(format!(
            "roadeep-offline-incomplete-{}",
            uuid::Uuid::new_v4()
        ));
        safe_directory(&directory).unwrap();
        let root = directory.join("local-ai/v1");
        let progress_count = std::sync::atomic::AtomicUsize::new(0);
        let result = install_from(
            &root,
            Arc::new(AtomicU64::new(1)),
            1,
            |_| {
                progress_count.fetch_add(1, Ordering::Relaxed);
            },
            Some(&directory),
        )
        .await;
        assert!(result.is_err());
        assert!(!root.exists());
        assert_eq!(progress_count.load(Ordering::Relaxed), 0);
        fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    #[ignore = "Full isolated offline preparation; set ROADEEP_LOCAL_TEST_ROOT and ROADEEP_LOCAL_TEST_BUNDLE"]
    async fn install_full_offline_bundle() {
        let root = PathBuf::from(
            std::env::var_os("ROADEEP_LOCAL_TEST_ROOT").expect("explicit isolated root"),
        );
        let bundle = PathBuf::from(
            std::env::var_os("ROADEEP_LOCAL_TEST_BUNDLE")
                .expect("explicit bundled package directory"),
        );
        assert!(root.is_absolute() && bundle.is_absolute());
        assert_eq!(root.file_name().unwrap(), "v1");
        let paths = install_from(
            &root,
            Arc::new(AtomicU64::new(1)),
            1,
            |status| {
                assert_ne!(status.phase, "downloading");
                println!(
                    "offline {} {} {}/{}",
                    status.phase, status.component, status.downloaded, status.total
                );
            },
            Some(&bundle),
        )
        .await
        .unwrap();
        assert_eq!(paths.root, root);
        assert!(verify(&root).unwrap().is_some());
        assert!(read_enabled(&root).unwrap());
        assert!(
            !root.parent().unwrap().join("downloads").exists(),
            "Offline install must not create or rely on a downloads cache"
        );
        let timestamp = fs::metadata(&paths.brain_model)
            .unwrap()
            .modified()
            .unwrap();
        assert!(verify(&root).unwrap().is_some());
        assert_eq!(
            fs::metadata(&paths.brain_model)
                .unwrap()
                .modified()
                .unwrap(),
            timestamp,
            "Verification must preserve the existing installed model"
        );
    }
    #[tokio::test]
    #[ignore = "Explicit full package metadata and corruption rejection; no network or userdata"]
    async fn corrupt_offline_bundle_is_rejected_without_cache_fallback() {
        let bundle = PathBuf::from(
            std::env::var_os("ROADEEP_LOCAL_TEST_BUNDLE")
                .expect("explicit bundled package directory"),
        );
        let base = bundle
            .parent()
            .unwrap()
            .join(format!("offline-corrupt-test-{}", uuid::Uuid::new_v4()));
        let bad = base.join("packages");
        safe_directory(&bad).unwrap();
        for (index, asset) in assets::all().iter().enumerate() {
            let source = bundle.join(asset.cache);
            let target = bad.join(asset.cache);
            if index == 0 {
                fs::copy(&source, &target).unwrap();
                let mut file = fs::OpenOptions::new().write(true).open(target).unwrap();
                file.write_all(b"BAD!").unwrap();
            } else {
                fs::hard_link(source, target).unwrap();
            }
        }
        let root = base.join("local-ai/v1");
        let error = install_from(
            &root,
            Arc::new(AtomicU64::new(1)),
            1,
            |status| assert_ne!(status.phase, "downloading"),
            Some(&bad),
        )
        .await
        .unwrap_err();
        assert!(error.contains("integrity"), "{error}");
        assert!(!root.exists());
        fs::remove_dir_all(&base).unwrap();
    }
    #[test]
    fn windows_reparse_points_are_rejected_even_without_symlink_flag() {
        assert!(unsafe_link(false, 0x400));
        assert!(unsafe_link(false, 0x410));
        assert!(unsafe_link(true, 0));
        assert!(!unsafe_link(false, 0x10));
    }
    #[test]
    fn config_is_bounded_and_saved_atomically() {
        let root =
            std::env::temp_dir().join(format!("roadeep-runtime-test-{}", uuid::Uuid::new_v4()));
        safe_directory(&root).unwrap();
        write_enabled(&root, true).unwrap();
        assert!(read_enabled(&root).unwrap());
        write_enabled(&root, false).unwrap();
        assert!(!read_enabled(&root).unwrap());
        fs::write(
            root.join("enabled.json"),
            b"true followed by unexpected content",
        )
        .unwrap();
        assert!(read_enabled(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn rejects_unsafe_archive_paths() {
        for p in [
            "",
            "../secret",
            "/absolute",
            "C:/host",
            "folder/../x",
            "folder\\x",
        ] {
            assert!(relative(p).is_err(), "{p}");
        }
        assert!(relative("lib/engine.dll").is_ok());
    }
    #[test]
    fn cancelled_generation_is_rejected() {
        let c = AtomicU64::new(3);
        assert!(check(&c, 3).is_ok());
        assert!(check(&c, 2).is_err());
    }
    #[test]
    fn rejects_receipt_traversal() {
        assert!(relative("../../outside.exe").is_err());
    }
    #[tokio::test]
    #[ignore = "Explicit full install from pinned cache; set ROADEEP_LOCAL_TEST_ROOT"]
    async fn install_pinned_cache() {
        let root = std::env::var("ROADEEP_LOCAL_TEST_ROOT").expect("explicit test root required");
        let root = PathBuf::from(root);
        assert!(root.is_absolute());
        assert_eq!(root.file_name().unwrap(), "v1");
        safe_directory(root.parent().unwrap()).unwrap();
        let paths = super::install_at(&root, Arc::new(AtomicU64::new(1)), 1, |s| {
            eprintln!("{} {} {}/{}", s.phase, s.component, s.downloaded, s.total)
        })
        .await
        .unwrap();
        assert!(paths.llama_exe.is_file());
        assert!(paths.llama_cpu_exe.is_file());
        assert!(paths.piper_espeak_data.is_dir());
        assert!(verify(&root).unwrap().is_some());
        assert!(read_enabled(&root).unwrap());
    }
    #[test]
    #[ignore = "Pinned official speaker archive in explicit assets directory; isolated extraction and corruption checks"]
    fn speaker_archive_extract_verify_and_corruption() {
        let assets = std::path::PathBuf::from(std::env::var("ROADEEP_LOCAL_TEST_ASSETS").unwrap());
        assert!(assets.is_absolute());
        let archive = assets.join("speaker-engine.tar.bz2");
        let output =
            std::env::temp_dir().join(format!("roadeep-speaker-archive-{}", uuid::Uuid::new_v4()));
        safe_directory(&output).unwrap();
        extract(&archive, &output, &AtomicU64::new(1), 1).unwrap();
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 3);
        verify_archive(&archive, &output).unwrap();
        let dll = output.join("sherpa-onnx-c-api.dll");
        let mut file = std::fs::OpenOptions::new().write(true).open(dll).unwrap();
        file.write_all(b"bad").unwrap();
        drop(file);
        assert!(verify_archive(&archive, &output).is_err());
        std::fs::remove_dir_all(&output).unwrap();
    }
}
