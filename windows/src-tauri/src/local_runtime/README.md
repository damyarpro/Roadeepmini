# App-managed local runtime

Only the settings window can install or cancel preparation. The island and
settings windows can inspect readiness and explicitly enable the runtime. No command accepts a URL, executable or host file path.
Installation runs in a background task; download bytes and component names are
reported through `local-runtime-progress` at most four times per second, with
immediate terminal states. Models are never loaded at app startup.

Storage is `%LOCALAPPDATA%/com.roadeep.desktop/local-ai/v1`. Assets are pinned
in `assets.rs` to release or repository revisions, exact sizes and SHA-256.
Downloads use HTTPS, bounded sizes, connection/read deadlines and cancellation.
Archives reject traversal, absolute paths, symlinks, duplicate files, excessive
entries and excessive expanded sizes. A complete staging directory is committed
only after every asset is verified; replacement failure restores the old runtime.
Prior versions and this install's staging directory alone are eligible for cleanup.

On the first status request in an app session the installation is hashed in a
blocking worker. Every model is checked against the compiled pins. Retained
archives are pinned too, and extracted executable/DLL contents are compared to
the archives, rather than trusting a mutable local receipt. Later status calls
use the verified cache and do not hash 1.6 GB repeatedly. Restart checks integrity
again. Unprepared status defaults disabled; a new complete installation enables
the runtime. Reinstallation or automatic speaker-model migration preserves the existing enabled preference. No credential is required for local inference.

The production installer also accepts an internal download cache under
`local-ai/downloads`, using the exact filenames in `assets.rs`. Cached bytes are
checked with the same size/hash contract; invalid cached bytes fail visibly.
The explicit ignored integration test `install_pinned_cache` runs the identical
installer against `ROADEEP_LOCAL_TEST_ROOT` (an absolute `.../local-ai/v1` path).
It is intended for release verification after those pinned assets are downloaded.

## Sources and licenses

- llama.cpp b11388 (MIT): <https://github.com/ggml-org/llama.cpp>
- Qwen3.5-2B (Apache-2.0); Unsloth GGUF conversion from the pinned repository:
  <https://huggingface.co/unsloth/Qwen3.5-2B-GGUF>
- whisper.cpp b5130 and Whisper models (MIT):
  <https://github.com/ggml-org/whisper.cpp>
- Piper 2023.11.14-2 (MIT): <https://github.com/rhasspy/piper>
- The separately downloaded Piper runtime includes eSpeak NG (GPL-3.0):
  <https://github.com/espeak-ng/espeak-ng>. Keep its notices and corresponding
  source availability when redistributing runtime binaries. The full offline installer
  retains engine archives, model assets and notices as application resources.
- Persian Amir voice dataset is CC0, as recorded in the pinned `MODEL_CARD`
  installed under `licenses/Piper-voice-MODEL_CARD`:
  <https://huggingface.co/rhasspy/piper-voices/tree/main/fa/fa_IR/amir/medium>

The UI must distinguish downloaded/verified/ready states. Provider errors are
reported without response bodies; logs contain outcomes, never prompts or audio.
Running engine processes belongs to `local_intelligence`, not this installer.
# Full offline installer

When `resource_dir/local-ai-packages` is present, it is the exclusive installation source. Its pinned package filenames are taken from `assets.rs`; missing, incomplete or corrupt files fail explicitly without falling back to the downloads cache or network. No HTTP client is constructed for this branch. A successful full installation uses the same checked staging, receipt and atomic commit as the online installer.

On first launch or first native status request, the bundled app prepares a missing per-user runtime asynchronously. Existing verified installations and their enabled preferences are preserved. Preparation emits `verifying` and `installing` progress, supports cancellation and explicit retry, and performs no model inference. If preparation is cancelled, subsequent status polls do not restart it automatically.

The ignored `install_full_offline_bundle` harness takes `ROADEEP_LOCAL_TEST_BUNDLE` and an isolated `ROADEEP_LOCAL_TEST_ROOT` ending in `v1`. It checks complete verification, enabled configuration, absence of a downloads cache, and preservation of installed model modification time during subsequent verification. `corrupt_offline_bundle_is_rejected_without_cache_fallback` uses independent corrupted package bytes in a disposable test directory and verifies that integrity failure never starts a download.


The speaker runtime retains the original pinned official tar.bz2 archive. Native bounded tar extraction installs only the three required Sherpa/ONNX DLLs in an isolated speaker directory; verification compares their contents to that retained archive. Existing complete nine-component runtimes trigger bundled preparation when the two new speaker assets are absent. The dedicated speaker archive harness verifies extraction, exact DLL count and corruption rejection.
