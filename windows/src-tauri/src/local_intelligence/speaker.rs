//! Probabilistic administrator voice matching, not authentication or replay protection.
use super::{audio, process, ready, request_id, speaker_worker, LocalIntelligence};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
const MODEL_SHA: &str = "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b";
const SERVICE: &str = "Roadeep.AdministratorVoice";
#[derive(Default)]
pub struct SpeakerState {
    enrolling: AtomicBool,
    generation: AtomicU64,
    profile: Mutex<()>,
    enrollment_request: Mutex<Option<String>>,
    tickets: Mutex<HashMap<String, Ticket>>,
}
struct Ticket {
    hash: String,
    generation: u64,
    created: Instant,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u8,
    model: String,
    dimension: usize,
    vector: String,
    threshold: f32,
}
#[derive(Serialize)]
pub struct Status {
    enrolled: bool,
    enrolling: bool,
}
#[derive(Serialize)]
pub struct Verification {
    matched: bool,
    score: f32,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket: Option<String>,
}
fn access(window: &tauri::WebviewWindow, settings_only: bool) -> Result<(), String> {
    if window.label() == "settings" || (!settings_only && window.label() == "island") {
        Ok(())
    } else {
        Err("speaker-permission".into())
    }
}
fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, "administrator-reference")
        .map_err(|_| "speaker-profile-unavailable".into())
}
fn load() -> Result<Option<Profile>, String> {
    let bytes = match entry()?.get_secret() {
        Ok(bytes) => bytes,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(_) => return Err("speaker-profile-unavailable".into()),
    };
    decode_profile(&bytes).map(Some)
}
fn serialize_profile(profile: &Profile) -> Result<Vec<u8>, String> {
    profile_vector(profile)?;
    let bytes = serde_json::to_vec(profile).map_err(|_| "speaker-profile-invalid")?;
    if bytes.len() > 2400 {
        return Err("speaker-profile-limit".into());
    }
    Ok(bytes)
}
fn decode_profile(bytes: &[u8]) -> Result<Profile, String> {
    if bytes.len() > 2560 {
        return Err("speaker-profile-invalid".into());
    }
    // Older references used set_password, which stores UTF-16 on Windows.
    // Decode only its unambiguous ASCII-JSON prefix; arbitrary malformed UTF-8 is never a migration.
    let legacy = cfg!(windows) && bytes.starts_with(b"{\0") && bytes.len() % 2 == 0;
    let legacy_text;
    let json = if legacy {
        let units = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        legacy_text = String::from_utf16(&units).map_err(|_| "speaker-profile-invalid")?;
        legacy_text.as_bytes()
    } else {
        bytes
    };
    if json.len() > 2400 {
        return Err("speaker-profile-invalid".into());
    }
    let profile: Profile = serde_json::from_slice(json).map_err(|_| "speaker-profile-invalid")?;
    profile_vector(&profile)?;
    Ok(profile)
}

fn normalize(vector: &[f32]) -> Result<Vec<f32>, String> {
    if !(16..=512).contains(&vector.len()) || vector.iter().any(|v| !v.is_finite()) {
        return Err("speaker-embedding-invalid".into());
    }
    let norm = vector
        .iter()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm < 1e-9 {
        return Err("speaker-embedding-invalid".into());
    }
    Ok(vector.iter().map(|v| (*v as f64 / norm) as f32).collect())
}
fn cosine(a: &[f32], b: &[f32]) -> Result<f32, String> {
    if a.len() != b.len() {
        return Err("speaker-dimension-invalid".into());
    }
    let a = normalize(a)?;
    let b = normalize(b)?;
    Ok(a.iter()
        .zip(b)
        .map(|(a, b)| a * b)
        .sum::<f32>()
        .clamp(-1., 1.))
}
fn profile_vector(profile: &Profile) -> Result<Vec<f32>, String> {
    if profile.version != 1
        || profile.model != MODEL_SHA
        || !(16..=512).contains(&profile.dimension)
        || !profile.threshold.is_finite()
        || !(0.65..=0.82).contains(&profile.threshold)
    {
        return Err("speaker-profile-invalid".into());
    }
    let bytes = audio::decode(&profile.vector, 1024)?;
    if bytes.len() != profile.dimension * 2 {
        return Err("speaker-profile-invalid".into());
    }
    normalize(
        &bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32767.)
            .collect::<Vec<_>>(),
    )
}
fn calibrate(embeddings: &[Vec<f32>]) -> Result<Profile, String> {
    if embeddings.len() != 3 {
        return Err("speaker-enrollment-invalid".into());
    }
    let vectors = embeddings
        .iter()
        .map(|v| normalize(v))
        .collect::<Result<Vec<_>, _>>()?;
    if vectors.iter().any(|v| v.len() != vectors[0].len()) {
        return Err("speaker-dimension-invalid".into());
    }
    let mut consistency = 1f32;
    for i in 0..3 {
        for j in i + 1..3 {
            consistency = consistency.min(cosine(&vectors[i], &vectors[j])?);
        }
    }
    if consistency < 0.55 {
        return Err("speaker-enrollment-inconsistent".into());
    }
    let mean = normalize(
        &(0..vectors[0].len())
            .map(|index| vectors.iter().map(|v| v[index]).sum::<f32>() / 3.)
            .collect::<Vec<_>>(),
    )?;
    let mut packed = Vec::new();
    for value in &mean {
        packed.extend(((*value * 32767.).round() as i16).to_le_bytes());
    }
    Ok(Profile {
        version: 1,
        model: MODEL_SHA.into(),
        dimension: mean.len(),
        vector: audio::encode(&packed),
        threshold: (consistency - 0.10).clamp(0.65, 0.82),
    })
}
fn status(state: &SpeakerState) -> Result<Status, String> {
    let _lock = state.profile.lock().map_err(|_| "speaker-state")?;
    Ok(Status {
        enrolled: load()?.is_some(),
        enrolling: state.enrolling.load(Ordering::Acquire),
    })
}
fn emit(app: &tauri::AppHandle, state: &SpeakerState) -> Result<Status, String> {
    let status = status(state)?;
    app.emit("local-speaker-changed", &status)
        .map_err(|_| "speaker-event-unavailable")?;
    Ok(status)
}
fn reject(reason: &str) -> Verification {
    Verification {
        matched: false,
        score: 0.,
        reason: reason.into(),
        ticket: None,
    }
}
fn rejection_outcome(reason: &str) -> &'static str {
    match reason {
        "too-short" => "rejected-too-short",
        "not-enrolled" => "rejected-not-enrolled",
        "uncertain" => "rejected-uncertain",
        "different-speaker" => "rejected-different-speaker",
        "enrollment-active" => "rejected-enrollment-active",
        _ => "rejected",
    }
}
async fn embedding(
    paths: &crate::local_runtime::RuntimePaths,
    wav: &str,
    split: bool,
    cancelled: std::sync::Arc<AtomicBool>,
) -> Result<speaker_worker::Output, String> {
    let input = speaker_worker::Input {
        dll: paths.speaker_dll.clone(),
        model: paths.speaker_model.clone(),
        wav_base64: wav.into(),
        split,
    };
    let exe = std::env::current_exe().map_err(|_| "speaker-worker-unavailable")?;
    worker_process(&exe, &input, cancelled).await
}
async fn worker_process(
    exe: &std::path::Path,
    input: &speaker_worker::Input,
    cancelled: std::sync::Arc<AtomicBool>,
) -> Result<speaker_worker::Output, String> {
    let split = input.split;
    let input = serde_json::to_vec(input).map_err(|_| "speaker-input")?;
    let result = process::run(
        &exe,
        &["--internal-speaker-embedding".into()],
        &input,
        cancelled.clone(),
        Duration::from_secs(20),
    )
    .await?;
    if cancelled.load(Ordering::Acquire) {
        return Err("local-cancelled".into());
    }
    let output: speaker_worker::Output =
        serde_json::from_slice(&result).map_err(|_| "speaker-output-invalid")?;
    if output.embeddings.len() != if split { 3 } else { 1 } {
        return Err("speaker-output-invalid".into());
    }
    for vector in &output.embeddings {
        normalize(vector)?;
    }
    Ok(output)
}
#[tauri::command]
pub async fn speaker_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SpeakerState>,
) -> Result<Status, String> {
    access(&window, false)?;
    status(&state)
}
#[tauri::command]
pub async fn speaker_enrollment_begin(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SpeakerState>,
) -> Result<Status, String> {
    access(&window, true)?;
    {
        let _lock = state.profile.lock().map_err(|_| "speaker-state")?;
        state.enrolling.store(true, Ordering::Release);
        state.generation.fetch_add(1, Ordering::AcqRel);
        state.tickets.lock().map_err(|_| "speaker-state")?.clear();
    }
    app.state::<LocalIntelligence>().shutdown();
    emit(&app, &state)
}
#[tauri::command]
pub async fn speaker_enrollment_end(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SpeakerState>,
) -> Result<Status, String> {
    access(&window, true)?;
    {
        let _lock = state.profile.lock().map_err(|_| "speaker-state")?;
        state.enrolling.store(false, Ordering::Release);
        state.generation.fetch_add(1, Ordering::AcqRel);
    }
    if let Some(id) = state
        .enrollment_request
        .lock()
        .map_err(|_| "speaker-state")?
        .take()
    {
        app.state::<LocalIntelligence>().cancel(Some(&id));
    }
    emit(&app, &state)
}
#[tauri::command]
pub async fn speaker_enroll(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SpeakerState>,
    intelligence: tauri::State<'_, LocalIntelligence>,
    runtime: tauri::State<'_, crate::local_runtime::RuntimeState>,
    wav_base64: String,
    request_id: String,
) -> Result<Status, String> {
    access(&window, true)?;
    self::request_id(&request_id)?;
    if !state.enrolling.load(Ordering::Acquire) {
        return Err("speaker-enrollment-not-active".into());
    }
    let bytes = audio::decode(&wav_base64, 960_128)?;
    audio::wav(&bytes, true)?;
    if speaker_worker::samples(&bytes)?.len() < 8 * 16000 {
        return Err("speaker-enrollment-too-short".into());
    }
    let operation = intelligence.begin(&request_id)?;
    *state
        .enrollment_request
        .lock()
        .map_err(|_| "speaker-state")? = Some(request_id.clone());
    let generation = state.generation.load(Ordering::Acquire);
    let started = Instant::now();
    let result = async {
        let pcm = speaker_worker::samples(&bytes)?;
        speaker_worker::enrollment_segments(&pcm)?;
        let (paths, _guard) = ready(&app, &runtime).await?;
        let extracted = embedding(&paths, &wav_base64, true, operation.cancelled.clone()).await?;
        let profile = calibrate(&extracted.embeddings)?;
        let serialized = serialize_profile(&profile)?;
        {
            let _lock = state.profile.lock().map_err(|_| "speaker-state")?;
            if generation != state.generation.load(Ordering::Acquire)
                || !state.enrolling.load(Ordering::Acquire)
                || operation.cancelled.load(Ordering::Acquire)
            {
                return Err("local-cancelled".into());
            }
            entry()?
                .set_secret(&serialized)
                .map_err(|_| "speaker-profile-save")?;
            state.generation.fetch_add(1, Ordering::AcqRel);
            state.tickets.lock().map_err(|_| "speaker-state")?.clear();
        }
        emit(&app, &state)
    }
    .await;
    *state
        .enrollment_request
        .lock()
        .map_err(|_| "speaker-state")? = None;
    super::record(
        "speaker-enroll",
        &request_id,
        started,
        enrollment_outcome(&result),
    );
    result
}
fn enrollment_outcome(result: &Result<Status, String>) -> &str {
    match result {
        Ok(_) => "ok",
        Err(error)
            if matches!(
                error.as_str(),
                "speaker-enrollment-inconsistent"
                    | "speaker-enrollment-insufficient"
                    | "speaker-enrollment-too-short"
                    | "speaker-enrollment-invalid"
                    | "speaker-output-invalid"
                    | "speaker-embedding-invalid"
                    | "speaker-profile-save"
                    | "speaker-profile-unavailable"
                    | "speaker-profile-limit"
                    | "local-cancelled"
                    | "local-disabled"
                    | "local-runtime-unavailable"
                    | "local-engine-failed"
                    | "local-engine-timeout"
            ) =>
        {
            error
        }
        Err(_) => "failed",
    }
}
#[tauri::command]
pub async fn speaker_verify(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SpeakerState>,
    intelligence: tauri::State<'_, LocalIntelligence>,
    runtime: tauri::State<'_, crate::local_runtime::RuntimeState>,
    wav_base64: String,
    request_id: String,
) -> Result<Verification, String> {
    access(&window, false)?;
    self::request_id(&request_id)?;
    let started = Instant::now();
    let result: Result<Verification, String> = async {
        if state.enrolling.load(Ordering::Acquire) {
            return Ok(reject("enrollment-active"));
        }
        let bytes = audio::decode(&wav_base64, 960_128)?;
        audio::wav(&bytes, true)?;
        if speaker_worker::samples(&bytes)?.len() < 2 * 16000 {
            return Ok(reject("too-short"));
        }
        let (profile, generation) = {
            let _lock = state.profile.lock().map_err(|_| "speaker-state")?;
            let Some(profile) = load()? else {
                return Ok(reject("not-enrolled"));
            };
            (profile, state.generation.load(Ordering::Acquire))
        };
        let operation = intelligence.begin(&request_id)?;
        let (paths, _guard) = ready(&app, &runtime).await?;
        let extracted = embedding(&paths, &wav_base64, false, operation.cancelled.clone()).await?;
        let score = cosine(&profile_vector(&profile)?, &extracted.embeddings[0])?.clamp(0., 1.);
        let matched = score >= profile.threshold + 0.03;
        if generation != state.generation.load(Ordering::Acquire)
            || state.enrolling.load(Ordering::Acquire)
            || operation.cancelled.load(Ordering::Acquire)
        {
            return Err("local-cancelled".into());
        }
        let ticket = if matched {
            let ticket = uuid::Uuid::new_v4().to_string();
            let mut tickets = state.tickets.lock().map_err(|_| "speaker-state")?;
            tickets.retain(|_, v| v.created.elapsed() < Duration::from_secs(30));
            if tickets.len() >= 8 {
                tickets.clear();
            }
            tickets.insert(
                ticket.clone(),
                Ticket {
                    hash: format!("{:x}", Sha256::digest(&bytes)),
                    generation,
                    created: Instant::now(),
                },
            );
            Some(ticket)
        } else {
            None
        };
        Ok(Verification {
            matched,
            score,
            reason: if matched {
                "matched"
            } else if score >= profile.threshold - 0.03 {
                "uncertain"
            } else {
                "different-speaker"
            }
            .into(),
            ticket,
        })
    }
    .await;
    let outcome = match &result {
        Ok(value) if value.matched => "matched",
        Ok(value) => rejection_outcome(&value.reason),
        Err(error) if error == "local-cancelled" => "cancelled",
        Err(_) => "failed",
    };
    super::record("speaker-verify", &request_id, started, outcome);
    result
}
pub(crate) fn consume_ticket(state: &SpeakerState, ticket: &str, wav: &[u8]) -> Result<(), String> {
    let mut tickets = state.tickets.lock().map_err(|_| "speaker-state")?;
    let ticket = tickets.remove(ticket).ok_or("speaker-ticket-invalid")?;
    if state.enrolling.load(Ordering::Acquire)
        || ticket.generation != state.generation.load(Ordering::Acquire)
        || ticket.created.elapsed() > Duration::from_secs(30)
        || ticket.hash != format!("{:x}", Sha256::digest(wav))
    {
        return Err("speaker-ticket-invalid".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn speaker_clear(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SpeakerState>,
) -> Result<(), String> {
    access(&window, true)?;
    {
        let _lock = state.profile.lock().map_err(|_| "speaker-state")?;
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(_) => return Err("speaker-profile-clear".into()),
        };
        state.generation.fetch_add(1, Ordering::AcqRel);
        state.enrolling.store(false, Ordering::Release);
        state.tickets.lock().map_err(|_| "speaker-state")?.clear();
    }
    app.state::<LocalIntelligence>().shutdown();
    super::assistant::clear_pending(&app)?;
    emit(&app, &state)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn reject_logs_only_known_reason_codes() {
        assert_eq!(super::rejection_outcome("too-short"), "rejected-too-short");
        assert_eq!(super::rejection_outcome("uncertain"), "rejected-uncertain");
        assert_eq!(super::rejection_outcome("different-speaker"), "rejected-different-speaker");
        assert_eq!(super::rejection_outcome("private text\n"), "rejected");
    }
    #[tokio::test]
    #[ignore = "Synthetic CAM++ full phrase acceptance and sparse name rejection; no real profile/audio/API"]
    async fn actual_synthetic_admin_wake_context() {
        use std::{path::PathBuf, sync::Arc};
        let assets = PathBuf::from(std::env::var("ROADEEP_LOCAL_TEST_ASSETS").unwrap());
        let exe = PathBuf::from(std::env::var("ROADEEP_SPEAKER_TEST_EXE").unwrap());
        assert!(assets.is_absolute() && exe.is_absolute());
        let paths = super::super::tests::downloaded_paths();
        async fn synthetic_pcm(paths: &crate::local_runtime::RuntimePaths, text: &str) -> Vec<f32> {
            let spoken = super::super::speak(paths, text, Arc::new(AtomicBool::new(false))).await.unwrap();
            let bytes = audio::decode(&spoken.wav_base64, 4 * 1024 * 1024).unwrap();
            let mut at = 12; let mut rate = 0; let mut samples = Vec::new();
            while at + 8 <= bytes.len() {
                let length = u32::from_le_bytes(bytes[at+4..at+8].try_into().unwrap()) as usize;
                let begin = at + 8;
                if &bytes[at..at+4] == b"fmt " { rate = u32::from_le_bytes(bytes[begin+4..begin+8].try_into().unwrap()); }
                if &bytes[at..at+4] == b"data" { samples = bytes[begin..begin+length].chunks_exact(2).map(|c| i16::from_le_bytes([c[0],c[1]]) as f32 / 32768.).collect::<Vec<_>>(); }
                at = begin + length + length % 2;
            }
            assert!(rate > 0);
            let count = samples.len() * 16000 / rate as usize;
            (0..count).map(|i| { let position = i as f64 * rate as f64 / 16000.; let left = position as usize; let f = (position-left as f64) as f32; samples[left]*(1.-f)+samples.get(left+1).copied().unwrap_or(samples[left])*f }).collect()
        }
        fn wave(samples: &[f32]) -> Vec<u8> {
            let length = (samples.len()*2) as u32; let mut out = Vec::new();
            out.extend(b"RIFF");out.extend((36+length).to_le_bytes());out.extend(b"WAVEfmt ");out.extend(16u32.to_le_bytes());out.extend(1u16.to_le_bytes());out.extend(1u16.to_le_bytes());out.extend(16000u32.to_le_bytes());out.extend(32000u32.to_le_bytes());out.extend(2u16.to_le_bytes());out.extend(16u16.to_le_bytes());out.extend(b"data");out.extend(length.to_le_bytes());
            for sample in samples { out.extend(((*sample*32768.).round().clamp(-32768.,32767.) as i16).to_le_bytes()); } out
        }
        let enrollment = synthetic_pcm(&paths, "سلام، امروز می‌خواهم درباره برنامه کاری صحبت کنم. فردا چند کار مهم دارم که باید انجام بدهم. لطفا برای خرید وسایل خانه و خواندن کتاب به من کمک کن. این چند جمله نمونه صدای من برای دستیار هستند.").await;
        assert!(enrollment.len() >= 8*16000);
        let mut input = speaker_worker::Input {
            dll: assets.join("speaker-engine/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-no-tts/lib/sherpa-onnx-c-api.dll"),
            model: assets.join("speaker-model.onnx"), wav_base64: audio::encode(&wave(&enrollment)), split: true,
        };
        let reference = worker_process(&exe,&input,Arc::new(AtomicBool::new(false))).await.unwrap();
        let profile = calibrate(&reference.embeddings).unwrap();
        for text in ["رودیپ یادداشت کن فردا آب بخورم", "رودیپ"] {
            let mut pcm = synthetic_pcm(&paths,text).await;
            // This fixture synthesizes a quiet tail; production captures every sample.
            pcm.resize(pcm.len().max(2*16000),0.);
            input.wav_base64 = audio::encode(&wave(&pcm));input.split = false;
            let candidate = worker_process(&exe,&input,Arc::new(AtomicBool::new(false))).await.unwrap();
            let score = cosine(&profile_vector(&profile).unwrap(),&candidate.embeddings[0]).unwrap();
            println!("Synthetic short wake accepted={} similarity={score:.4} threshold={:.4}",score>=profile.threshold+0.03,profile.threshold+0.03);
            if text.contains("یادداشت") {
                assert!(score>=profile.threshold+0.03,"Full synthetic phrase must retain enough speaker context");
            } else {
                assert!(score<profile.threshold+0.03,"A sparse synthetic name must remain rejected; no threshold bypass");
            }
        }
    }
    use super::*;
    #[test]
    fn finite_normalization_and_dimension_guards() {
        assert!(normalize(&vec![0.; 512]).is_err());
        assert!(normalize(&vec![f32::NAN; 512]).is_err());
        assert!(normalize(&vec![1.; 513]).is_err());
        assert!(cosine(&vec![1.; 512], &vec![1.; 192]).is_err());
    }
    #[test]
    fn enrollment_profile_is_compact_and_consistent() {
        let vectors = vec![vec![1.; 512]; 3];
        let profile = calibrate(&vectors).unwrap();
        assert!(serde_json::to_string(&profile).unwrap().len() < 2400);
        assert!(cosine(&profile_vector(&profile).unwrap(), &vectors[0]).unwrap() > 0.999);
        assert!(calibrate(&[vec![1.; 512], vec![-1.; 512], vec![1.; 512]]).is_err());
    }
    #[test]
    fn tickets_are_bound_single_use_and_invalidated() {
        let state = SpeakerState::default();
        state.tickets.lock().unwrap().insert(
            "ticket".into(),
            Ticket {
                hash: format!("{:x}", Sha256::digest(b"audio")),
                generation: 0,
                created: Instant::now(),
            },
        );
        assert!(consume_ticket(&state, "ticket", b"different").is_err());
        assert!(consume_ticket(&state, "ticket", b"audio").is_err());
        state.tickets.lock().unwrap().insert(
            "ticket".into(),
            Ticket {
                hash: format!("{:x}", Sha256::digest(b"audio")),
                generation: 0,
                created: Instant::now(),
            },
        );
        state.generation.store(1, Ordering::Release);
        assert!(consume_ticket(&state, "ticket", b"audio").is_err());
        for (name, age) in [("fresh", 0), ("expired", 31)] {
            state.tickets.lock().unwrap().insert(
                name.into(),
                Ticket {
                    hash: format!("{:x}", Sha256::digest(b"audio")),
                    generation: 1,
                    created: Instant::now() - Duration::from_secs(age),
                },
            );
        }
        assert!(consume_ticket(&state, "fresh", b"audio").is_ok());
        assert!(consume_ticket(&state, "fresh", b"audio").is_err());
        assert!(consume_ticket(&state, "expired", b"audio").is_err());
    }
    #[tokio::test]
    #[ignore = "Actual pinned CAM++ subprocess using public fixtures; explicit executable/assets paths, no user profile"]
    async fn actual_speaker_subprocess_matches_heldout_and_rejects_other() {
        use std::{path::PathBuf, sync::Arc};
        let assets = PathBuf::from(std::env::var("ROADEEP_LOCAL_TEST_ASSETS").unwrap());
        let exe = PathBuf::from(std::env::var("ROADEEP_SPEAKER_TEST_EXE").unwrap());
        assert!(assets.is_absolute() && exe.is_absolute());
        let dll = assets.join("speaker-engine/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-no-tts/lib/sherpa-onnx-c-api.dll");
        let model = assets.join("speaker-model.onnx");
        fn wave(samples: &[f32]) -> Vec<u8> {
            let length = (samples.len() * 2) as u32;
            let mut out = Vec::new();
            out.extend(b"RIFF");
            out.extend((36 + length).to_le_bytes());
            out.extend(b"WAVEfmt ");
            out.extend(16u32.to_le_bytes());
            out.extend(1u16.to_le_bytes());
            out.extend(1u16.to_le_bytes());
            out.extend(16000u32.to_le_bytes());
            out.extend(32000u32.to_le_bytes());
            out.extend(2u16.to_le_bytes());
            out.extend(16u16.to_le_bytes());
            out.extend(b"data");
            out.extend(length.to_le_bytes());
            for v in samples {
                out.extend(((*v * 32768.).round().clamp(-32768., 32767.) as i16).to_le_bytes());
            }
            out
        }
        let fixture = |name: &str| {
            speaker_worker::samples(
                &std::fs::read(assets.join("speaker-samples").join(name)).unwrap(),
            )
            .unwrap()
        };
        let mut enrollment = Vec::new();
        for name in ["spk1_snt1.wav", "spk1_snt2.wav", "spk1_snt3.wav"] {
            enrollment.extend(fixture(name));
        }
        let mut input = speaker_worker::Input {
            dll,
            model,
            wav_base64: audio::encode(&wave(&enrollment)),
            split: true,
        };
        let output = worker_process(&exe, &input, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        assert_eq!(output.embeddings[0].len(), 512);
        let mut profile = calibrate(&output.embeddings).unwrap();
        for (lead, tail) in [(0.0, 3.0), (2.5, 0.0), (3.0, 3.0)] {
            let mut paused = vec![0.; (lead * 16000.) as usize];
            paused.extend_from_slice(&enrollment);
            paused.resize(paused.len() + (tail * 16000.) as usize, 0.);
            input.wav_base64 = audio::encode(&wave(&paused));
            let paused_output = worker_process(&exe, &input, Arc::new(AtomicBool::new(false)))
                .await
                .unwrap();
            let mut consistency = 1f32;
            for i in 0..3 {
                for j in i + 1..3 {
                    consistency = consistency.min(
                        cosine(&paused_output.embeddings[i], &paused_output.embeddings[j]).unwrap(),
                    );
                }
            }
            profile = calibrate(&paused_output.embeddings)
                .expect("same speaker with boundary pauses enrolls");
            println!("public pause fixture lead={lead}s tail={tail}s consistency={consistency:.4} accepted=true");
        }
        input.split = false;
        for (name, expected) in [
            ("spk1_snt4.wav", true),
            ("spk2_snt1.wav", false),
            ("spk2_snt2.wav", false),
        ] {
            let mut pcm = fixture(name);
            if pcm.len() < 32000 {
                pcm.resize(32000, 0.);
            }
            input.wav_base64 = audio::encode(&wave(&pcm));
            let output = worker_process(&exe, &input, Arc::new(AtomicBool::new(false)))
                .await
                .unwrap();
            let score = cosine(&profile_vector(&profile).unwrap(), &output.embeddings[0]).unwrap();
            println!(
                "public fixture {name}: cosine={score:.4}, threshold={:.4}",
                profile.threshold + 0.03
            );
            assert_eq!(score >= profile.threshold + 0.03, expected);
        }
        let cancelled = Arc::new(AtomicBool::new(true));
        assert!(
            matches!(worker_process(&exe,&input,cancelled).await,Err(e) if e == "local-cancelled")
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = cancelled.clone();
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            trigger.store(true, Ordering::Release);
        });
        let started = Instant::now();
        assert!(
            matches!(worker_process(&exe,&input,cancelled).await,Err(e) if e == "local-cancelled")
        );
        cancel_task.await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(4));
        input.model = assets.join("missing-speaker.onnx");
        assert!(
            worker_process(&exe, &input, Arc::new(AtomicBool::new(false)))
                .await
                .is_err()
        );
    }
    #[test]
    fn profile_storage_fits_windows_blob_and_reads_legacy_without_migrating() {
        let profile = calibrate(&vec![vec![1.; 512]; 3]).unwrap();
        let bytes = serialize_profile(&profile).unwrap();
        assert!(bytes.len() <= 2400);
        assert!(
            String::from_utf8(bytes.clone())
                .unwrap()
                .encode_utf16()
                .count()
                * 2
                > 2560
        );
        assert_eq!(decode_profile(&bytes).unwrap().dimension, 512);
        assert!(decode_profile(&vec![b'a'; 2561]).is_err());
        assert!(decode_profile(b"{invalid").is_err());
        let old = calibrate(&vec![vec![1.; 192]; 3]).unwrap();
        let text = String::from_utf8(serialize_profile(&old).unwrap()).unwrap();
        let legacy: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(legacy.len() <= 2560);
        #[cfg(windows)]
        assert_eq!(decode_profile(&legacy).unwrap().dimension, 192);
    }
    #[test]
    #[ignore = "Explicit isolated temporary Windows credential roundtrip; no administrator profile or microphone"]
    fn isolated_profile_credential_raw_roundtrip() {
        let service = format!("Roadeep.AdministratorVoice.Test.{}", uuid::Uuid::new_v4());
        let credential = keyring::Entry::new(&service, "synthetic-reference").unwrap();
        struct Cleanup<'a>(&'a keyring::Entry);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = self.0.delete_credential();
            }
        }
        let _cleanup = Cleanup(&credential);
        let profile = calibrate(&vec![vec![1.; 512]; 3]).unwrap();
        let bytes = serialize_profile(&profile).unwrap();
        credential
            .set_secret(&bytes)
            .expect("isolated raw credential save");
        let restored = credential
            .get_secret()
            .expect("isolated raw credential read");
        assert_eq!(restored, bytes);
        assert_eq!(decode_profile(&restored).unwrap().dimension, 512);
        let old = calibrate(&vec![vec![1.; 192]; 3]).unwrap();
        let old = String::from_utf8(serialize_profile(&old).unwrap()).unwrap();
        credential
            .set_password(&old)
            .expect("isolated legacy credential save");
        assert_eq!(
            decode_profile(&credential.get_secret().unwrap())
                .unwrap()
                .dimension,
            192
        );
        credential
            .delete_credential()
            .expect("isolated credential cleanup");
        assert!(matches!(
            credential.get_secret(),
            Err(keyring::Error::NoEntry)
        ));
    }
    #[test]
    fn enrollment_error_metadata_is_allowlisted_without_arbitrary_details() {
        assert_eq!(
            enrollment_outcome(&Err("speaker-enrollment-inconsistent".into())),
            "speaker-enrollment-inconsistent"
        );
        assert_eq!(
            enrollment_outcome(&Err("private path and microphone contents".into())),
            "failed"
        );
        assert_eq!(
            enrollment_outcome(&Err("speaker-enrollment-inconsistent secret detail".into())),
            "failed"
        );
    }
}
