// Every IPC command gets an `allow-<name>` permission; capabilities grant them
// per window (ARCHITECTURE §3.1, least privilege). The test harness exists
// only in debug builds.
const COMMANDS: &[&str] = &[
    "app_ready",
    "app_log",
    "app_copy_diagnostics",
    "app_open_logs_folder",
    "app_open_external",
    "app_quit",
    "commands_registry",
    "redact_text",
    "deeplink_pending",
    "window_toggle_maximize",
    "window_minimize",
    "window_close",
    "settings_get_all",
    "settings_set",
    "settings_reset",
    "auth_sign_in",
    "auth_cancel",
    "auth_sign_out",
    "auth_state",
    "auth_continue_offline",
    "entitlement_get",
    "entitlement_refresh",
    "billing_open",
    "telemetry_capture",
    "workspace_pick_folder",
    "workspace_add",
    "workspace_list",
    "workspace_rename",
    "workspace_remove",
    "workspace_locate",
    "workspace_set_flags",
    "workspace_reveal",
    "rail_move",
    "tab_create",
    "tab_rename",
    "tab_close",
    "tab_reorder",
    "layout_get",
    "layout_save",
    "pane_create",
    "pane_list",
    "pane_close",
    "pane_move",
    "pty_spawn",
    "pty_attach",
    "pty_write",
    "pty_ack",
    "pty_resize",
    "pty_kill",
    "term_set_theme_colors",
    "orphans_list",
    "orphans_resolve",
];

const TEST_COMMANDS: &[&str] = &[
    "test_counters",
    "test_spawn_log",
    "test_clock_advance",
    "test_auth_refresh",
];

fn main() {
    let debug = std::env::var("PROFILE").as_deref() == Ok("debug");
    let mut all: Vec<&'static str> = COMMANDS.to_vec();
    if debug {
        all.extend_from_slice(TEST_COMMANDS);
    }
    // AppManifest wants a 'static list; a build script can leak it.
    let all: &'static [&'static str] = Box::leak(all.into_boxed_slice());
    let attrs =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(all));
    if let Err(err) = tauri_build::try_build(attrs) {
        panic!("tauri build failed: {err:#}");
    }
}
