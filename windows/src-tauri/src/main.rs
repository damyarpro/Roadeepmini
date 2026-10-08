// Roadeep runs without a console window: the island is the whole UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--internal-speaker-embedding") {
        std::process::exit(roadeep_lib::internal_speaker_worker());
    }
    roadeep_lib::run()
}
