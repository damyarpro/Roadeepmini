pub struct Asset {
    pub id: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub destination: &'static str,
    pub archive: bool,
    pub cache: &'static str,
}
pub fn total() -> u64 {
    all().iter().map(|a| a.size).sum()
}
pub fn all() -> &'static [Asset] {
    &ASSETS
}
const ASSETS:[Asset;11]=[
Asset{id:"llama-vulkan",url:"https://github.com/ggml-org/llama.cpp/releases/download/b11388/llama-b11388-bin-win-vulkan-x64.zip",size:33282695,sha256:"0cc41db2e39901a3361ff8a1354e93a21cb1f4196768c261fbcbba2c33231458",destination:"llama",archive:true,cache:"llama-vulkan.zip"},
Asset{id:"llama-cpu",url:"https://github.com/ggml-org/llama.cpp/releases/download/b11388/llama-b11388-bin-win-cpu-x64.zip",size:19363391,sha256:"679d167cd9d59d539a721fbc34305a1fea365ced1f4d0f87f4ccde3d4a403a7e",destination:"llama-cpu",archive:true,cache:"llama-cpu.zip"},
Asset{id:"whisper",url:"https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-x64.zip",size:8573270,sha256:"f9ec6c52a2e949b62ab51fa21d0d497958f9e41c3010c157c4e42932d5316f3c",destination:"whisper",archive:true,cache:"whisper.zip"},
Asset{id:"piper",url:"https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",size:22477236,sha256:"f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",destination:"piper",archive:true,cache:"piper.zip"},
Asset{id:"brain",url:"https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-Q4_K_M.gguf",size:1280835840,sha256:"aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",destination:"models/brain.gguf",archive:false,cache:"brain.gguf"},
Asset{id:"speech-recognition",url:"https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin",size:190085487,sha256:"ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",destination:"models/whisper.bin",archive:false,cache:"whisper-model.bin"},
Asset{id:"persian-voice",url:"https://huggingface.co/rhasspy/piper-voices/resolve/c10ece1aade47bb51c153c893d14e5bf8e5b7117/fa/fa_IR/amir/medium/fa_IR-amir-medium.onnx",size:63531379,sha256:"fb815380d969ea372b0b21b0de14421f58fe481047e153e69685d079b6e1a9d1",destination:"models/fa.onnx",archive:false,cache:"piper-model.onnx"},
Asset{id:"voice-config",url:"https://huggingface.co/rhasspy/piper-voices/resolve/c10ece1aade47bb51c153c893d14e5bf8e5b7117/fa/fa_IR/amir/medium/fa_IR-amir-medium.onnx.json",size:4958,sha256:"75f918a3bf0f57a9179abe725af529f2a5c79d6c899e2a84aec76c685d5dfb9a",destination:"models/fa.onnx.json",archive:false,cache:"piper-config.json"},
Asset{id:"voice-license",url:"https://huggingface.co/rhasspy/piper-voices/resolve/c10ece1aade47bb51c153c893d14e5bf8e5b7117/fa/fa_IR/amir/medium/MODEL_CARD",size:264,sha256:"0d7d315699945ac830162f82d485d79af51a33081a7649e8992726bd4bba2384",destination:"licenses/Piper-voice-MODEL_CARD",archive:false,cache:"piper-MODEL_CARD"},
Asset{id:"speaker-engine",url:"https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-no-tts.tar.bz2",size:23271851,sha256:"4b0a94f7b5c606b1b64a19a831c2127559e4b3d34e195465ebc7be73d9ed4783",destination:"speaker",archive:true,cache:"speaker-engine.tar.bz2"},
Asset{id:"speaker-model",url:"https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",size:29596978,sha256:"357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",destination:"models/speaker.onnx",archive:false,cache:"speaker-model.onnx"},
];

#[cfg(test)]
mod tests {
    #[test]
    fn immutable_whisper_model_url_matches_repository_root() {
        let asset = super::all()
            .iter()
            .find(|a| a.id == "speech-recognition")
            .unwrap();
        assert!(asset
            .url
            .ends_with("/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin"));
        assert!(!asset.url.contains("/models/"));
    }
}
