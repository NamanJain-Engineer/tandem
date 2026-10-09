//! `tandem_app`: the desktop app's services and the Tauri wiring around them.
//! Services take their dependencies explicitly so they run in `cargo test`
//! without a window (ARCHITECTURE §2.12).

pub mod browser;
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
            commands::shell::sidebar_get,
            commands::shell::sidebar_save,
            commands::terminal::pane_rename,
            commands::shell::focus_source,
            commands::shell::approvals_pending,
            commands::shell::approval_resolve,
            commands::shell::status_snapshot,
            commands::shell::status_mark_seen,
            commands::shell::dashboard_close_idle,
            commands::shell::notifications_list,
            commands::shell::notifications_mark_read,
            commands::shell::ui_visibility,
            commands::restore::restore_get_snapshot,
            commands::restore::restore_resume,
            commands::restore::restore_start_fresh,
            commands::restore::restore_cancel,
            commands::restore::restore_scrollback,
            commands::restore::restore_auto_resume,
            commands::updates::updates_state,
            commands::updates::updates_check_now,
            commands::updates::updates_set_channel,
            commands::updates::updates_install,
            commands::app::app_licenses,
            commands::onboarding::onboarding_get,
            commands::onboarding::onboarding_set,
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
            commands::launch::presets_list,
            commands::launch::preset_save,
            commands::launch::preset_delete,
            commands::launch::launch_lineup,
            commands::launch::launch_retry_seat,
            commands::handoff::handoff_brief,
            commands::handoff::handoff_start,
            commands::handoff::handoff_to_thread,
            commands::lanes::lane_create,
            commands::lanes::lane_diff,
            commands::lanes::lane_merge,
            commands::lanes::lane_create_pr,
            commands::lanes::lane_remove,
            commands::lanes::lanes_sweep,
            commands::files::fs_list,
            commands::files::fs_read,
            commands::files::fs_write,
            commands::files::fs_compare,
            commands::files::fs_create,
            commands::files::fs_rename,
            commands::files::fs_delete,
            commands::files::fs_watch,
            commands::files::fs_unwatch,
            commands::checkpoints::checkpoints_list,
            commands::checkpoints::checkpoint_create,
            commands::checkpoints::checkpoint_diff,
            commands::checkpoints::checkpoint_restore,
            commands::checkpoints::checkpoints_usage,
            commands::checkpoints::checkpoints_delete_all,
            commands::recall::history_search,
            commands::recall::history_run,
            commands::recall::history_delete,
            commands::recall::history_clear,
            commands::files::editor_tabs_get,
            commands::files::editor_tabs_save,
            commands::files::app_reveal_path,
            commands::git::git_status,
            commands::git::git_diff,
            commands::git::git_stage,
            commands::git::git_unstage,
            commands::git::git_discard,
            commands::git::git_commit,
            commands::git::git_push,
            commands::git::git_pull,
            commands::git::git_branches,
            commands::git::git_switch,
            commands::git::git_create_branch,
            commands::browser::browser_open,
            commands::browser::browser_navigate,
            commands::browser::browser_command,
            commands::browser::browser_set_bounds,
            commands::browser::browser_set_device,
            commands::browser::browser_move,
            commands::browser::browser_picker,
            commands::browser::browser_picker_brief,
            commands::browser::browser_close,
            commands::browser::browser_clear_data,
            commands::workspaces::workspace_locate,
            commands::workspaces::workspace_set_flags,
            commands::workspaces::workspace_reveal,
            commands::workspaces::rail_move,
            commands::workspaces::tab_create,
            commands::workspaces::tab_rename,
            commands::workspaces::tab_close,
            commands::workspaces::tab_reorder,
            commands::workspaces::layout_get,
            commands::workspaces::layout_save,
            commands::terminal::pane_create,
            commands::terminal::pane_list,
            commands::terminal::pane_close,
            commands::terminal::pane_move,
            commands::terminal::pty_spawn,
            commands::terminal::pty_attach,
            commands::terminal::pty_write,
            commands::terminal::pty_ack,
            commands::terminal::pty_resize,
            commands::terminal::pty_kill,
            commands::terminal::term_set_theme_colors,
            commands::terminal::orphans_list,
            commands::terminal::orphans_resolve,
            commands::terminal::sessions_saved_list,
            commands::terminal::term_quote_paths,
            commands::terminal::pane_resolve_link,
            commands::terminal::term_last_output,
            commands::terminal::term_stage_clipboard_image,
            commands::terminal::pty_type_prompt,
            commands::terminal::status_list,
            commands::engines::engines_list,
            commands::engines::engines_detect,
            commands::engines::engines_install_command,
            commands::engines::engines_set_extra_args,
            commands::engines::accounts_list,
            commands::accounts::accounts_begin_add,
            commands::accounts::accounts_begin_session,
            commands::accounts::accounts_finish_login,
            commands::accounts::accounts_cancel_login,
            commands::accounts::accounts_update,
            commands::accounts::accounts_set_default,
            commands::accounts::accounts_remove,
            commands::accounts::accounts_status,
            commands::accounts::accounts_overrides,
            commands::accounts::workspace_set_account_override,
            commands::accounts::usage_get,
            commands::accounts::usage_refresh,
            commands::limits::limits_list,
            commands::limits::limit_resume_at_reset,
            commands::limits::limit_resume_now,
            commands::limits::limit_cancel_resume,
            commands::limits::limit_dismiss,
            commands::limits::limit_continue,
            commands::limits::contexts_list,
            commands::threads::thread_create,
            commands::threads::thread_list,
            commands::threads::thread_get,
            commands::threads::thread_events,
            commands::threads::thread_subscribe,
            commands::threads::thread_send,
            commands::threads::thread_stop,
            commands::threads::thread_retry,
            commands::threads::thread_update,
            commands::threads::thread_archive,
            commands::threads::thread_delete,
            commands::threads::thread_switch_engine,
            commands::threads::thread_move_to_folder,
            commands::threads::thread_save_draft,
            commands::threads::thread_attach_paths,
            commands::threads::thread_attach_clipboard_image,
            commands::threads::attachment_remove,
            commands::threads::thread_mention_search,
            commands::threads::models_list,
            commands::threads::thread_pick_files,
            commands::threads::thread_unqueue,
            commands::threads::thread_implement_plan,
            commands::threads::thread_changes,
            commands::threads::thread_file_read,
            commands::engines::engines_report,
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

pub(crate) fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The window background for a theme setting: `--tn-bg` from UI_SPEC §1.2
/// (slate-950 dark, slate-50 light). `os_light` resolves "system".
pub fn theme_background(theme: &str, os_light: bool) -> (u8, u8, u8) {
    let light = match theme {
        "light" => true,
        "system" => os_light,
        _ => false,
    };
    if light {
        (0xf1, 0xf5, 0xf9)
    } else {
        (0x06, 0x0a, 0x12)
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
    let os_light = matches!(window.theme(), Ok(tauri::Theme::Light));
    let (r, g, b) = theme_background(&theme, os_light);
    let _ = window.set_background_color(Some(tauri::window::Color(r, g, b, 0xff)));
}

/// Saves the window state shortly after it changes, so a crash or a killed
/// process keeps the last size, position and maximized state (F-SH-01 AC2).
fn persist_window_state(app: &tauri::App) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tauri_plugin_window_state::AppHandleExt;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let generation = Arc::new(AtomicU64::new(0));
    let handle = app.handle().clone();
    window.on_window_event(move |event| {
        if !matches!(
            event,
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)
        ) {
            return;
        }
        let mine = generation.fetch_add(1, Ordering::AcqRel) + 1;
        let (generation, handle) = (generation.clone(), handle.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            if generation.load(Ordering::Acquire) == mine {
                if let Err(err) = handle.save_window_state(StateFlags::all() & !StateFlags::VISIBLE)
                {
                    tracing::debug!(error = %err, "couldn't save the window state");
                }
            }
        });
    });
}

/// A screen-space rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl ScreenRect {
    fn contains(&self, other: &ScreenRect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x as i64 + other.w as i64 <= self.x as i64 + self.w as i64
            && other.y as i64 + other.h as i64 <= self.y as i64 + self.h as i64
    }
}

/// True when the window lies entirely on one monitor (F-SH-01 AC3).
pub fn fully_on_screen(window: ScreenRect, monitors: &[ScreenRect]) -> bool {
    monitors.iter().any(|m| m.contains(&window))
}

/// Re-centres a restored window that isn't fully on any display (a saved
/// position from a monitor that's gone).
fn ensure_on_screen(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_maximized().unwrap_or(false) {
        return;
    }
    let (Ok(pos), Ok(size), Ok(monitors)) = (
        window.outer_position(),
        window.outer_size(),
        window.available_monitors(),
    ) else {
        return;
    };
    let rect = ScreenRect {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    };
    let screens: Vec<ScreenRect> = monitors
        .iter()
        .map(|m| ScreenRect {
            x: m.position().x,
            y: m.position().y,
            w: m.size().width,
            h: m.size().height,
        })
        .collect();
    if !screens.is_empty() && !fully_on_screen(rect, &screens) {
        tracing::info!(
            code = "window.recentred",
            "saved window position is off-screen; centring"
        );
        let _ = window.center();
    }
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
    // Checkpoints past the settings' limits go at startup (F-EX-01).
    let checkpoints = state.checkpoints.clone();
    tauri::async_runtime::spawn(async move {
        checkpoints.prune_all().await;
    });
    // Command recall keeps its retention at startup and hourly (F-EX-02).
    let recall = state.recall.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            recall.prune().await;
        }
    });
    let staging = state.data.staging();
    tauri::async_runtime::spawn_blocking(move || {
        services::staging::sweep(&staging, services::staging::KEEP_FOR)
    });
    // Live panes' scrollback is snapshotted every 5 minutes so a crash keeps
    // something to show on restore (§5.4).
    let terminals = state.terminals.clone();
    let snapshots = state.data.scrollback();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(services::terminal::snapshot_every());
        tick.tick().await;
        loop {
            tick.tick().await;
            let (t, dir) = (terminals.clone(), snapshots.clone());
            let _ = tokio::task::spawn_blocking(move || t.snapshot_all(&dir)).await;
        }
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
            eprintln!("Vibezeo couldn't start: {err}");
            std::process::exit(1);
        }
    };
    services::telemetry::install_panic_hook(&state.data.crashes());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");
    let initial_links = deeplink::from_args(std::env::args().skip(1));
    let window_state_file = state.data.window_state().to_string_lossy().into_owned();
    let e2e = state.e2e;
    let endpoints_api = state.endpoints.api.clone();

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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
        commands::browser::test_browser_eval,
        commands::test::test_spawn_log,
        commands::test::test_clock_advance,
        commands::test::test_fixture_agent,
        commands::test::test_notifier_calls,
        commands::test::test_notifier_click,
        commands::test::test_set_window_focused,
        commands::test::test_thread_seed,
        commands::test::test_thread_state,
        commands::test::test_thread_seed_events,
        commands::test::test_update_installed,
        commands::test::test_min_version_shift,
        commands::test::test_auth_refresh,
        commands::test::test_account_create,
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(app_commands![]);

    let mut context = tauri::generate_context!();
    // The local test feed is plain HTTP on loopback (debug builds only).
    let feed = services::updates::feed_base(&endpoints_api);
    if services::updates::allows_insecure_feed(&feed) {
        if let Some(cfg) = context.config_mut().plugins.0.get_mut("updater") {
            cfg["dangerousInsecureTransportProtocol"] = serde_json::json!(true);
        }
    }
    let result = builder
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            paint_theme_background(app, &state);
            if let Some(window) = app.get_webview_window("main") {
                window.set_title("Vibezeo")?;
                // The browser's layer goes in before the window first shows
                // (app_ready shows it): a WebKitGTK view moved after it has
                // painted stays blank (D-206).
                #[cfg(target_os = "linux")]
                browser::wry_host::install_overlay(&window);
            }
            {
                let profile = state.data.browser_profile();
                let host_app = handle.clone();
                let service = browser::BrowserService::new(move |policy, sink| {
                    std::sync::Arc::new(browser::wry_host::WryHost::new(
                        host_app, profile, policy, sink,
                    ))
                });
                let emit_app = handle.clone();
                let keep = commands::browser::keep_pane_pages(state.store.clone());
                service.set_emitter(std::sync::Arc::new(move |s: &browser::BrowserState| {
                    keep(s);
                    let _ = emit_app.emit_to("main", "browser:state", s);
                }));
                let picked_app = handle.clone();
                service.set_picker_emitter(std::sync::Arc::new(
                    move |p: &browser::picker::BrowserPicked| {
                        let _ = picked_app.emit_to("main", "browser:picker", p);
                    },
                ));
                let _ = state.browser.set(service);
            }
            services::events::forward(handle.clone(), &state.bus);
            ensure_on_screen(app);
            persist_window_state(app);
            spawn_background(&state);
            state.approvals.start();
            // Updates (F-SH-06): the signed feed, if this build has a key.
            if let Some(key) = services::updates::pubkey(state::BUNDLED_KEYS) {
                let record = state
                    .e2e
                    .then(|| state.data.root().join("test-update-installed"));
                state.updates.set_backend(std::sync::Arc::new(
                    services::updates::tauri_backend::TauriUpdater::new(
                        handle.clone(),
                        feed.clone(),
                        key,
                        record,
                    ),
                ));
            }
            {
                let store = state.store.clone();
                tauri::async_runtime::spawn(state.updates.clone().run_schedule(move || {
                    let store = store.clone();
                    Box::pin(async move {
                        tandem_store::repo::settings::get(
                            &store,
                            tandem_core::settings::SettingKey::AutoCheckUpdates,
                        )
                        .await
                        .ok()
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                    })
                }));
            }
            // OS notifications: the system's, or a recording one under the
            // test harness (F-DB-03).
            {
                use crate::services::notifier::{Notifier, RecordingNotifier, SystemNotifier};
                use std::sync::Arc;
                let (notifier, recording): (Arc<dyn Notifier>, Option<Arc<RecordingNotifier>>) =
                    if state.e2e {
                        let r = Arc::new(RecordingNotifier::default());
                        (r.clone(), Some(r))
                    } else {
                        (Arc::new(SystemNotifier::new(handle.clone())), None)
                    };
                state
                    .notifications
                    .start(handle.clone(), notifier, recording);
            }
            // The status server, then engines: the cached report at once, the
            // remote registry and a fresh detection in the background (F-EN-01).
            let engines_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = engines_state.state::<AppState>();
                state.terminals.status.start(&state.data.run()).await;
                state.terminals.engines.load_cached().await;
                let config = state.config.refresh().await;
                state.terminals.engines.apply_config(&config);
                let login = state.terminals.login_env().await;
                state.terminals.engines.detect(&login).await;
            });
            // Threads on the Dashboard from the start (D-6).
            let threads_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                threads_state.state::<AppState>().threads.announce().await;
            });
            // Remote config (model catalog, registry overrides) has a 10-min
            // TTL: a model added remotely shows up without an update (F-TH-08 AC1).
            let config_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(services::config::CONFIG_TTL);
                tick.tick().await;
                loop {
                    tick.tick().await;
                    let state = config_state.state::<AppState>();
                    let config = state.config.refresh().await;
                    state.terminals.engines.apply_config(&config);
                }
            });
            // Worktree sweep: at startup, then every 6 h (F-CD-15). It never
            // deletes anything; finished worktrees become one notification.
            let sweep_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(services::lanes::SWEEP_EVERY);
                loop {
                    tick.tick().await;
                    let state = sweep_state.state::<AppState>();
                    if let Err(err) = services::lanes::sweep_and_notify(&state).await {
                        tracing::info!(error = %err, "worktree sweep skipped");
                    }
                }
            });
            // Detect again when the window comes back, at most once a minute.
            if let Some(window) = app.get_webview_window("main") {
                let focus = handle.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(focused) = event {
                        focus
                            .state::<AppState>()
                            .notifications
                            .set_focused(*focused);
                    }
                    if let tauri::WindowEvent::Focused(true) = event {
                        let focus = focus.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = focus.state::<AppState>();
                            if state.terminals.engines.focus_due() {
                                let login = state.terminals.login_env().await;
                                state.terminals.engines.detect(&login).await;
                            }
                        });
                    }
                });
            }
            let telemetry_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = telemetry_state.state::<AppState>();
                state.apply_settings().await;
                state.apply_orphan_policy().await;
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
        .build(context);
    match result {
        // However the app ends (Quit, last window closed), panes get their
        // scrollback snapshot and every process tree is stopped.
        Ok(app) => app.run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                let state = handle.state::<AppState>();
                state.terminals.shutdown(&state.data.scrollback());
            }
        }),
        Err(err) => {
            tracing::error!(error = %err, "tauri runtime failed");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_sh_05_1_dark_on_a_light_os_starts_hidden_on_the_dark_background() {
        // Dark stays dark on a light OS; only "system" follows the OS.
        assert_eq!(theme_background("dark", true), (0x06, 0x0a, 0x12));
        assert_eq!(theme_background("light", false), (0xf1, 0xf5, 0xf9));
        assert_eq!(theme_background("system", true), (0xf1, 0xf5, 0xf9));
        assert_eq!(theme_background("system", false), (0x06, 0x0a, 0x12));
        // The window is created hidden on the dark background and only shown
        // by `app_ready` after the UI's first paint.
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("conf");
        let main = &conf["app"]["windows"][0];
        assert_eq!(main["visible"], false);
        assert_eq!(main["backgroundColor"], "#060a12");
        let html = include_str!("../../index.html");
        assert!(
            html.contains(r#"data-theme="dark""#),
            "the page starts in the dark theme before React runs"
        );
    }

    #[test]
    fn f_sh_01_3_off_screen_windows_are_detected() {
        let primary = ScreenRect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let second = ScreenRect {
            x: 1920,
            y: 0,
            w: 1280,
            h: 1024,
        };
        let monitors = [primary, second];
        assert!(fully_on_screen(
            ScreenRect {
                x: 100,
                y: 100,
                w: 1440,
                h: 900
            },
            &monitors
        ));
        assert!(fully_on_screen(
            ScreenRect {
                x: 2000,
                y: 50,
                w: 1000,
                h: 700
            },
            &monitors
        ));
        // Partly off the edge, or on a monitor that's gone.
        assert!(!fully_on_screen(
            ScreenRect {
                x: 1776,
                y: 990,
                w: 1440,
                h: 900
            },
            &[primary]
        ));
        assert!(!fully_on_screen(
            ScreenRect {
                x: 20000,
                y: 20000,
                w: 1000,
                h: 700
            },
            &monitors
        ));
        // Spanning two monitors isn't "fully on" either one.
        assert!(!fully_on_screen(
            ScreenRect {
                x: 1500,
                y: 0,
                w: 1000,
                h: 700
            },
            &monitors
        ));
    }
}

#[cfg(test)]
mod harness_tests {
    /// The E2E harness never ships: no `allow-test-*` permission in the
    /// shipped capability, and the harness capability is added only at
    /// runtime in debug builds with TANDEM_E2E=1 (M2 exit checklist).
    #[test]
    fn release_capabilities_have_no_test_harness() {
        let main = include_str!("../capabilities/main.json");
        assert!(
            !main.contains("allow-test-"),
            "test harness permission in capabilities/main.json"
        );
        let harness = include_str!("../test-capability.json");
        assert!(harness.contains("allow-test-auth-refresh"));
        let lib = include_str!("lib.rs");
        assert!(
            lib.contains("if e2e {\n                app.add_capability"),
            "the harness capability is gated on e2e"
        );
        let commands = include_str!("commands/mod.rs");
        assert!(
            commands.contains("#[cfg(debug_assertions)]\npub mod test;"),
            "the harness module is debug-only"
        );
    }

    /// Every registered command has an `allow-<name>` permission (build.rs)
    /// that the window's capability grants; otherwise the webview's call is
    /// refused at runtime (ARCHITECTURE §3.1).
    #[test]
    fn every_registered_command_is_permitted() {
        let lib = include_str!("lib.rs");
        let build = include_str!("../build.rs");
        let main = include_str!("../capabilities/main.json");
        let harness = include_str!("../test-capability.json");
        let names: Vec<&str> = lib
            .lines()
            .filter_map(|l| l.trim().strip_prefix("commands::")?.strip_suffix(','))
            .filter_map(|path| path.split("::").nth(1))
            .collect();
        assert!(names.len() > 100, "found {} commands", names.len());
        for name in names {
            assert!(
                build.contains(&format!("\"{name}\",")),
                "{name} is missing from build.rs COMMANDS"
            );
            let allow = format!("\"allow-{}\"", name.replace('_', "-"));
            let granted = if name.starts_with("test_") {
                harness
            } else {
                main
            };
            assert!(
                granted.contains(&allow),
                "{allow} is missing from the capability"
            );
        }
    }
}
