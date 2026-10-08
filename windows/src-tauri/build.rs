use std::fmt::Write as _;
use std::path::Path;

fn main() {
    // Unit tests that instantiate the real Tauri runtime boundary also need
    // CommonControls v6, just like the packaged application's manifest.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
    embed_catalog();
    tauri_build::build()
}

/// Bundles every `catalog/*.json` manifest into the binary: OUT_DIR gets a
/// `catalog_files.rs` holding `(file name, include_str!(…))` pairs, sorted by
/// name so builds are reproducible. Parsing and validation happen at runtime
/// (catalog/mod.rs), where a bad manifest is logged and skipped, and in the
/// `catalog` tests, where it fails the build.
fn embed_catalog() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog");
    // A directory path makes Cargo rescan its whole contents for changes.
    println!("cargo:rerun-if-changed=catalog");

    let mut files: Vec<(String, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_json = path.extension().is_some_and(|e| e == "json");
            if !is_json || !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            files.push((name, path.to_string_lossy().to_string()));
        }
    }
    files.sort();

    let mut out = String::from("pub static FILES: &[(&str, &str)] = &[\n");
    for (name, path) in &files {
        // `{:?}` writes a valid Rust string literal, backslashes and all.
        let _ = writeln!(out, "    ({name:?}, include_str!({path:?})),");
    }
    out.push_str("];\n");

    let target = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("catalog_files.rs");
    std::fs::write(target, out).expect("write catalog_files.rs");
}
