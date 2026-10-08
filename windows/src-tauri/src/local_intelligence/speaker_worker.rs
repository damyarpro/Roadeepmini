//! One-shot, pinned Sherpa C ABI worker. It never starts Tauri or opens a network connection.
use serde::{Deserialize, Serialize};
use std::{
    ffi::{c_char, c_void, CString},
    io::{Read, Write},
    path::PathBuf,
};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub dll: PathBuf,
    pub model: PathBuf,
    pub wav_base64: String,
    pub split: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Output {
    pub embeddings: Vec<Vec<f32>>,
}
pub fn entry() -> i32 {
    let result = (|| -> Result<(), String> {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(1_400_001)
            .read_to_end(&mut bytes)
            .map_err(|_| "speaker-input")?;
        if bytes.len() > 1_400_000 {
            return Err("speaker-input-limit".into());
        }
        let input: Input = serde_json::from_slice(&bytes).map_err(|_| "speaker-input")?;
        let output = extract(&input)?;
        serde_json::to_writer(std::io::stdout(), &output).map_err(|_| "speaker-output")?;
        std::io::stdout().flush().map_err(|_| "speaker-output")?;
        Ok(())
    })();
    if result.is_ok() {
        0
    } else {
        eprintln!("speaker-worker-failed");
        1
    }
}
pub(crate) fn samples(bytes: &[u8]) -> Result<Vec<f32>, String> {
    super::audio::wav(bytes, true)?;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            return Ok(bytes[at + 8..at + 8 + len]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                .collect());
        }
        at += 8 + len + len % 2;
    }
    Err("speaker-audio-invalid".into())
}
fn trim_quiet_boundaries(samples: &[f32]) -> Option<&[f32]> {
    const FRAME: usize = 320;
    const CONTEXT: usize = 1600;
    let signal = |frame: &[f32]| {
        frame.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / frame.len() as f64
            >= 0.002f64.powi(2)
    };
    let first = samples.chunks(FRAME).position(signal)?;
    let last = samples
        .chunks(FRAME)
        .enumerate()
        .filter(|(_, frame)| signal(frame))
        .map(|(index, _)| index)
        .last()?;
    let start = (first * FRAME).saturating_sub(CONTEXT);
    let end = ((last + 1) * FRAME + CONTEXT).min(samples.len());
    Some(&samples[start..end])
}
/// Boundary energy is signal quality, not speech recognition; internal pauses remain intact.
pub(crate) fn enrollment_segments(samples: &[f32]) -> Result<Vec<&[f32]>, String> {
    if samples.len() > 30 * 16000 || samples.iter().any(|v| !v.is_finite() || v.abs() > 1.) {
        return Err("speaker-audio-invalid".into());
    }
    let trimmed = trim_quiet_boundaries(samples).ok_or("speaker-enrollment-insufficient")?;
    let mut segments = Vec::with_capacity(3);
    for index in 0..3 {
        let window = &trimmed[index * trimmed.len() / 3..(index + 1) * trimmed.len() / 3];
        let useful = trim_quiet_boundaries(window).ok_or("speaker-enrollment-insufficient")?;
        if useful.len() < 2 * 16000 {
            return Err("speaker-enrollment-insufficient".into());
        }
        segments.push(useful);
    }
    Ok(segments)
}

#[cfg(windows)]
#[repr(C)]
struct Config {
    model: *const c_char,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
}
#[cfg(windows)]
struct Library(*mut c_void);
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
#[cfg(windows)]
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
#[cfg(windows)]
impl Library {
    unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, String> {
        let address = GetProcAddress(self.0, name.as_ptr().cast());
        if address.is_null() {
            return Err("speaker-abi-mismatch".into());
        }
        Ok(std::mem::transmute_copy(&address))
    }
}
#[cfg(windows)]
pub(crate) fn extract(input: &Input) -> Result<Output, String> {
    use std::os::windows::ffi::OsStrExt;
    let bytes = super::audio::decode(&input.wav_base64, 960_128)?;
    let samples = samples(&bytes)?;
    if samples.len() < 16000 * 2 {
        return Err("speaker-too-short".into());
    }
    if input.split && samples.len() < 16000 * 8 {
        return Err("speaker-enrollment-too-short".into());
    }
    let dll = input
        .dll
        .canonicalize()
        .map_err(|_| "speaker-runtime-missing")?;
    if dll.file_name().and_then(|n| n.to_str()) != Some("sherpa-onnx-c-api.dll") {
        return Err("speaker-runtime-path".into());
    }
    let metadata = std::fs::symlink_metadata(&input.dll).map_err(|_| "speaker-runtime-path")?;
    use std::os::windows::fs::MetadataExt;
    if metadata.file_attributes() & 0x400 != 0 || metadata.len() != 3249664 {
        return Err("speaker-runtime-path".into());
    }
    let model = input
        .model
        .canonicalize()
        .map_err(|_| "speaker-model-missing")?;
    let metadata = std::fs::symlink_metadata(&input.model).map_err(|_| "speaker-model-missing")?;
    if metadata.file_attributes() & 0x400 != 0 || metadata.len() != 29596978 {
        return Err("speaker-model-invalid".into());
    }
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    // Explicit DLL directory and system directories only; no PATH or current-directory DLL search.
    let library =
        Library(unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x100 | 0x800) });
    if library.0.is_null() {
        return Err("speaker-library-load".into());
    }
    let model = CString::new(
        model
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned(),
    )
    .map_err(|_| "speaker-model-path")?;
    let cpu = CString::new("cpu").unwrap();
    unsafe {
        let create: unsafe extern "C" fn(*const Config) -> *const c_void =
            library.symbol(b"SherpaOnnxCreateSpeakerEmbeddingExtractor\0")?;
        let destroy: unsafe extern "C" fn(*const c_void) =
            library.symbol(b"SherpaOnnxDestroySpeakerEmbeddingExtractor\0")?;
        let dim: unsafe extern "C" fn(*const c_void) -> i32 =
            library.symbol(b"SherpaOnnxSpeakerEmbeddingExtractorDim\0")?;
        let stream: unsafe extern "C" fn(*const c_void) -> *const c_void =
            library.symbol(b"SherpaOnnxSpeakerEmbeddingExtractorCreateStream\0")?;
        let stream_destroy: unsafe extern "C" fn(*const c_void) =
            library.symbol(b"SherpaOnnxDestroyOnlineStream\0")?;
        let accept: unsafe extern "C" fn(*const c_void, i32, *const f32, i32) =
            library.symbol(b"SherpaOnnxOnlineStreamAcceptWaveform\0")?;
        let finish: unsafe extern "C" fn(*const c_void) =
            library.symbol(b"SherpaOnnxOnlineStreamInputFinished\0")?;
        let ready: unsafe extern "C" fn(*const c_void, *const c_void) -> i32 =
            library.symbol(b"SherpaOnnxSpeakerEmbeddingExtractorIsReady\0")?;
        let compute: unsafe extern "C" fn(*const c_void, *const c_void) -> *const f32 =
            library.symbol(b"SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding\0")?;
        let free: unsafe extern "C" fn(*const f32) =
            library.symbol(b"SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding\0")?;
        let extractor = create(&Config {
            model: model.as_ptr(),
            num_threads: 2,
            debug: 0,
            provider: cpu.as_ptr(),
        });
        if extractor.is_null() {
            return Err("speaker-extractor-failed".into());
        }
        let result = (|| -> Result<Output, String> {
            let dimension = dim(extractor);
            if !(16..=512).contains(&dimension) {
                return Err("speaker-dimension-invalid".into());
            }
            let segments = if input.split {
                enrollment_segments(&samples)?
            } else {
                vec![samples.as_slice()]
            };
            let mut embeddings = Vec::new();
            for chunk in segments {
                let stream = stream(extractor);
                if stream.is_null() {
                    return Err("speaker-stream-failed".into());
                }
                accept(stream, 16000, chunk.as_ptr(), chunk.len() as i32);
                finish(stream);
                if ready(extractor, stream) == 0 {
                    stream_destroy(stream);
                    return Err("speaker-too-short".into());
                }
                let vector = compute(extractor, stream);
                stream_destroy(stream);
                if vector.is_null() {
                    return Err("speaker-embedding-failed".into());
                }
                let values = std::slice::from_raw_parts(vector, dimension as usize).to_vec();
                free(vector);
                if values.iter().any(|f| !f.is_finite()) {
                    return Err("speaker-embedding-invalid".into());
                }
                embeddings.push(values);
            }
            Ok(Output { embeddings })
        })();
        destroy(extractor);
        result
    }
}
#[cfg(not(windows))]
pub(crate) fn extract(_: &Input) -> Result<Output, String> {
    Err("speaker-platform-unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_pauses_trim_without_removing_internal_pauses() {
        let mut signal = vec![0.; 16000 * 3];
        signal.extend(vec![0.05; 16000 * 9]);
        signal.extend(vec![0.; 16000 * 3]);
        let segments = enrollment_segments(&signal).unwrap();
        assert_eq!(segments.len(), 3);
        assert!(segments
            .iter()
            .all(|s| s.len() >= 16000 * 2 && s.len() < 16000 * 4));
        assert!(enrollment_segments(&vec![0.; 16000 * 12]).is_err());
        assert!(enrollment_segments(&vec![0.05; 16000 * 3]).is_err());
        assert!(enrollment_segments(&vec![f32::NAN; 16000 * 12]).is_err());
        assert!(enrollment_segments(&vec![0.05; 16000 * 31]).is_err());
        let mut paused = vec![0.05; 16000];
        paused.extend(vec![0.; 16000]);
        paused.extend(vec![0.05; 16000]);
        assert_eq!(trim_quiet_boundaries(&paused).unwrap().len(), paused.len());
    }
}
