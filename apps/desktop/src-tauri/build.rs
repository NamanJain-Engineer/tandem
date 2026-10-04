// Every IPC command gets an `allow-<name>` permission; capabilities grant them
// per window (ARCHITECTURE §3.1, least privilege).
const COMMANDS: &[&str] = &[
    "app_ready",
    "app_log",
    "app_copy_diagnostics",
    "commands_registry",
    "redact_text",
    "settings_get_all",
    "settings_set",
    "settings_reset",
];

fn main() {
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(err) = tauri_build::try_build(attrs) {
        panic!("tauri build failed: {err:#}");
    }
}
