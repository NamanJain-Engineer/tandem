//! `tandem_app`: the desktop app's services and the Tauri wiring around them.
//! Services take their dependencies explicitly so they run in `cargo test`
//! without a window (ARCHITECTURE §2.12).

pub mod commands;
pub mod services;
pub mod state;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_window_state::StateFlags;

use crate::services::auth::Callback;
use crate::services::deeplink::{self, DeepLink};
use crate::state::AppState;

/// Every command, plus `$extra` (the test harness in debug builds).
macro_rules! app_commands {
    ($($extra:path),* $(,)?) => {
        tauri::generate_handler![
            commands::app::app_ready,
            commands::app::app_log,
            commands::app::app_copy_diagnostics,
            commands::app::app_open_logs_folder,
            commands::app::app_open_external,
            commands::app::app_quit,
            commands::app::commands_registry,
            commands::app::redact_text,
            commands::app::deeplink_pending,
            commands::window::window_toggle_maximize,
            commands::window::window_minimize,
            commands::window::window_close,
            commands::settings::settings_get_all,
            commands::settings::settings_set,
            commands::settings::settings_reset,
            commands::account::auth_sign_in,
            commands::account::auth_cancel,
            commands::account::auth_sign_out,
            commands::account::auth_state,
            commands::account::auth_continue_offline,
            commands::account::entitlement_get,
            commands::account::entitlement_refresh,
            commands::account::billing_open,
            commands::account::telemetry_capture,
            commands::workspaces::workspace_pick_folder,
            commands::workspaces::workspace_add,
            commands::workspaces::workspace_list,
            commands::workspaces::workspace_rename,
            commands::workspaces::workspace_remove,
            commands::workspaces::tab_create,
            commands::workspaces::tab_rename,
            commands::workspaces::tab_close,
            commands::workspaces::tab_reorder,
            commands::terminal::pane_create,
            commands::terminal::pane_list,
            commands::terminal::pane_close,
            commands::terminal::pty_spawn,
            commands::terminal::pty_attach,
            commands::terminal::pty_write,
            commands::terminal::pty_ack,
            commands::terminal::pty_resize,
            commands::terminal::pty_kill,
            commands::terminal::term_set_theme_colors,
            $($extra),*
        ]
    };
}

/// Routes one `tandem://` link (F-SH-13). Sign-in callbacks go straight to
/// the auth service; everything else is for the UI, queued until it's ready.
pub fn route_link(app: &AppHandle, raw: &str) {
    let state = app.state::<AppState>();
    let link = deeplink::parse(raw).unwrap_or_else(|| {
        tracing::warn!(
            code = "deeplink.unrecognized",
            len = raw.len(),
            "unrecognized link"
        );
        DeepLink::unrecognized()
    });
    focus_main(app);
    if link.route == "auth/callback" {
        let (Some(code), Some(st)) = (link.params.get("code"), link.params.get("state")) else {
            return;
        };
        state.auth.handle_callback(Callback {
            code: code.clone(),
            state: st.clone(),
        });
        return;
    }
    if state.deeplinks.offer(link.clone()) {
        let _ = app.emit_to("main", "app:deep-link", link);
    }
}

fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Sets the native background from the persisted theme before the webview
/// paints, so there's no white flash (F-SH-05).
fn paint_theme_background(app: &tauri::App, state: &AppState) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let theme = tandem_store::repo::settings::get_blocking(
        &state.store,
        tandem_core::settings::SettingKey::Theme,
    )
    .ok()
    .and_then(|v| v.as_str().map(str::to_string))
    .unwrap_or_else(|| "dark".into());
    let light = match theme.as_str() {
        "light" => true,
        "system" => matches!(window.theme(), Ok(tauri::Theme::Light)),
        _ => false,
    };
    let color = if light {
        tauri::window::Color(0xf8, 0xfa, 0xfc, 0xff)
    } else {
        tauri::window::Color(0x0b, 0x11, 0x1c, 0xff)
    };
    let _ = window.set_background_color(Some(color));
}

fn spawn_background(state: &AppState) {
    let auth = state.auth.clone();
    tauri::async_runtime::spawn(auth.clone().run_refresher());
    tauri::async_runtime::spawn(auth.run_resume_watcher());
    let ent = state.entitlements.clone();
    tauri::async_runtime::spawn(ent.clone().run_auth_listener());
    tauri::async_runtime::spawn(ent.clone().run_events());
    tauri::async_runtime::spawn(async move {
        ent.refresh().await;
    });
    let telemetry = state.telemetry.clone();
    tauri::async_runtime::spawn(telemetry.clone().run_uploader());
    let terminals = state.terminals.clone();
    tauri::async_runtime::spawn(async move {
        terminals.login_env().await;
    });
    let _ = telemetry;
}

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
    services::telemetry::install_panic_hook(&state.data.crashes());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");
    let initial_links = deeplink::from_args(std::env::args().skip(1));
    let window_state_file = state.data.window_state().to_string_lossy().into_owned();
    let e2e = state.e2e;

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            focus_main(app);
            for raw in deeplink::from_args(args.into_iter().skip(1)) {
                route_link(app, &raw);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_filename(window_state_file)
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .manage(state);
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(app_commands![
        commands::test::test_counters,
        commands::test::test_spawn_log,
        commands::test::test_clock_advance,
        commands::test::test_auth_refresh,
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(app_commands![]);

    let result = builder
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            paint_theme_background(app, &state);
            if let Some(window) = app.get_webview_window("main") {
                window.set_title("Tandem")?;
            }
            services::events::forward(handle.clone(), &state.bus);
            spawn_background(&state);
            let telemetry_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = telemetry_state.state::<AppState>();
                state.apply_settings().await;
                state.telemetry.upload_crashes().await;
            });
            if e2e {
                app.add_capability(include_str!("../test-capability.json"))?;
            }
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let links = handle.clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        route_link(&links, url.as_str());
                    }
                });
                #[cfg(any(target_os = "linux", windows))]
                if !e2e {
                    let _ = app.deep_link().register(services::auth::deep_link_scheme());
                }
            }
            for raw in &initial_links {
                route_link(&handle, raw);
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(err) = result {
        tracing::error!(error = %err, "tauri runtime failed");
        std::process::exit(1);
    }
}
