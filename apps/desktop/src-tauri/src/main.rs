// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started by ssh as its askpass helper (F-RM-01): answer and leave.
    if let Some(code) = tandem_app::services::askpass::client_main() {
        std::process::exit(code);
    }
    tandem_app::run();
}
