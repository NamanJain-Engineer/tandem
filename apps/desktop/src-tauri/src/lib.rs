//! `tandem_app`: the desktop app's services and the Tauri wiring around them.
//! Services take their dependencies explicitly so they run in `cargo test`
//! without a window (ARCHITECTURE §2.12).

pub mod commands;
pub mod services;
pub mod state;

use tauri::Manager;

use crate::state::AppState;

/// Builds and runs the Tauri app. Startup follows ARCHITECTURE §1.3: data dir
/// and logging first, then the store, then the hidden main window, which the
/// UI shows on first paint via `app_ready`.
pub fn run() {
    let state = match AppState::init() {
        Ok(state) => state,
        Err(err) => {
            eprintln!("Tandem couldn't start: {err}");
            std::process::exit(1);
        }
    };
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");

    let result = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::app::app_ready,
            commands::app::app_log,
            commands::app::app_copy_diagnostics,
            commands::app::commands_registry,
            commands::app::redact_text,
            commands::settings::settings_get_all,
            commands::settings::settings_set,
            commands::settings::settings_reset,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                window.set_title("Tandem")?;
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(err) = result {
        tracing::error!(error = %err, "tauri runtime failed");
        std::process::exit(1);
    }
}
