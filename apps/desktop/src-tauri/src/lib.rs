//! `tandem_app`: the desktop app's services and the Tauri wiring around them.
//! Services take their dependencies explicitly so they run in `cargo test`
//! without a window (ARCHITECTURE §2.12).

pub mod browser;
pub mod commands;
pub mod headless;
pub mod services;
pub mod state;
mod tray;

use tandem_core::supervisor;
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
            commands::app::app_diagnostics,
            commands::app::app_open_logs_folder,
            commands::app::app_open_external,
            commands::app::app_quit,
            commands::app::commands_registry,
            commands::app::redact_text,
            commands::app::deeplink_pending,
            commands::window::window_toggle_maximize,
            commands::window::window_minimize,
            commands::window::window_close,
            commands::routines::routines_list,
            commands::routines::routine_get,
            commands::routines::routine_preview_schedule,
            commands::routines::routine_save,
            commands::routines::routine_delete,
            commands::routines::routine_pause,
            commands::routines::routine_run_now,
            commands::routines::routine_runs,
            commands::routines::routines_background_set,
            commands::routines::routines_background_status,
            commands::tasks::tasks_meta,
            commands::tasks::tasks_list,
            commands::tasks::task_get,
            commands::tasks::task_save,
            commands::tasks::task_comment,
            commands::tasks::task_archive,
            commands::tasks::task_restore,
            commands::tasks::task_connections_list,
            commands::tasks::task_connection_revoke,
            commands::task_exec::task_execute,
            commands::task_exec::task_attempts_list,
            commands::task_exec::task_attempt_stop,
            commands::task_exec::task_attempt_resume,
            commands::task_exec::task_attempt_retry,
            commands::teammates::teammate_list,
            commands::teammates::teammate_get,
            commands::teammates::teammate_create,
            commands::teammates::teammate_update,
            commands::teammates::teammate_delete,
            commands::teammates::teammate_duplicate,
            commands::teammates::teammate_reorder,
            commands::teammates::memory_get,
            commands::teammates::memory_write,
            commands::teammates::memory_undo,
            commands::teammates::skills_list,
            commands::teammates::skill_get,
            commands::teammates::skill_save,
            commands::teammates::skill_remove,
            commands::teammates::skills_library,
            commands::teammates::skill_add_from_library,
            commands::teammates::skill_sync,
            commands::teammates::teammate_launch_in_terminal,
            commands::teammates::teammate_suggestions,
            commands::threads::thread_reorder,
            commands::teammates::teammate_draft_with_ai,
            commands::teammates::teammate_templates,
            commands::teammates::memory_suggestions_resolve,
            commands::teammates::mail_list,
            commands::teammates::mail_get,
            commands::teammates::mail_send,
            commands::teammates::mail_retry,
            commands::plugins::plugins_catalog,
            commands::plugins::grants_list,
            commands::plugins::grants_set,
            commands::plugins::plugin_connect_oauth,
            commands::plugins::plugin_connect_cancel,
            commands::plugins::plugin_connect_api_key,
            commands::plugins::plugin_connect_local_port,
            commands::plugins::plugin_disconnect,
            commands::plugins::plugin_detail,
            commands::plugins::plugin_set_option,
            commands::plugins::held_drafts_list,
            commands::plugins::held_draft_run,
            commands::plugins::held_drafts_discard,
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
            commands::remote::ssh_hosts_list,
            commands::remote::ssh_host_add,
            commands::remote::ssh_host_remove,
            commands::remote::ssh_connect,
            commands::remote::ssh_hostkey_decide,
            commands::remote::ssh_askpass_reply,
            commands::remote::ssh_disconnect,
            commands::remote::ssh_list_dir,
            commands::remote::ssh_host_reset_key,
            commands::remote::wsl_distros,
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
            commands::lanes::lane_second_opinion,
            commands::review::review_queue,
            commands::review::review_approve,
            commands::review::review_unapprove,
            commands::review::review_snooze,
            commands::review::review_item_diff,
            commands::review::review_run_check,
            commands::review::review_repo_get,
            commands::review::review_repo_set,
            commands::lanes::lane_merge,
            commands::lanes::lane_overlap,
            commands::lanes::lane_forge,
            commands::lanes::lane_runtime,
            commands::lanes::lane_previews,
            commands::lanes::lane_preview_start,
            commands::lanes::merge_queue_plan,
            commands::lanes::merge_queue_start,
            commands::lanes::merge_queue_get,
            commands::lanes::merge_queue_stop,
            commands::lanes::merge_queue_undo,
            commands::lanes::pr_checks_allow,
            commands::lanes::lane_checks,
            commands::lanes::lane_check_failure,
            commands::lanes::lane_review_comments,
            commands::lanes::lane_radar,
            commands::lanes::lane_radar_note,
            commands::lanes::lane_create_prs,
            commands::lanes::lane_prs_get,
            commands::lanes::worktree_env_check,
            commands::lanes::lane_create_pr,
            commands::lanes::lane_remove,
            commands::lanes::worktree_setup_get,
            commands::lanes::worktree_setup_save,
            commands::lanes::worktree_setup_trust,
            commands::lanes::lane_setup,
            commands::lanes::lane_setup_run,
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
            commands::lsp::lsp_sync,
            commands::lsp::lsp_definition,
            commands::lsp::lsp_references,
            commands::lsp::lsp_hover,
            commands::lsp::lsp_trust,
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
            commands::ports::ports_list,
            commands::ports::ports_stop,
            commands::prompts::prompts_list,
            commands::prompts::prompts_render,
            commands::prompts::prompts_create,
            commands::prompts::prompts_share_repo,
            commands::bestof::bestof_start,
            commands::bestof::bestof_list,
            commands::bestof::bestof_get,
            commands::bestof::bestof_check,
            commands::bestof::bestof_check_stop,
            commands::bestof::bestof_keep,
            commands::bestof::bestof_discard,
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
            commands::terminal::pty_park,
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
            commands::scratchpad::scratchpad_list,
            commands::scratchpad::scratchpad_read,
            commands::scratchpad::scratchpad_write,
            commands::scratchpad::scratchpad_delete,
            commands::usage::usage_report,
            commands::usage::usage_share_save,
            commands::usage::usage_sessions,
            commands::runaway::runaway_list,
            commands::runaway::runaway_dismiss,
            commands::runaway::runaway_pause,
            commands::runaway::runaway_resume,
            commands::night_shift::night_accounts,
            commands::night_shift::night_list,
            commands::night_shift::night_schedule,
            commands::night_shift::night_cancel,
            commands::sandbox::sandbox_status,
            commands::sandbox::sandbox_panes,
            commands::data_controls::data_export,
            commands::data_controls::data_delete_local,
            commands::find_work::find_work,
            commands::conductor_persona::conductor_memory_get,
            commands::conductor_persona::conductor_memory_save,
            commands::conductor_persona::conductor_memory_clear,
            commands::conductor_persona::conductor_memory_resolve,
            commands::conductor_persona::conductor_wake_status,
            commands::terminal::term_last_output,
            commands::terminal::term_stage_clipboard_image,
            commands::terminal::pty_type_prompt,
            commands::terminal::status_list,
            commands::engines::engines_list,
            commands::engine_versions::engine_versions,
            commands::engine_versions::engine_check_update,
            commands::engine_versions::engine_pin,
            commands::engine_versions::engine_rollback_command,
            commands::engine_versions::engine_background,
            commands::engine_versions::engine_background_stop,
            commands::library::library_list,
            commands::library::library_transcript,
            commands::library::library_resume,
            commands::library::library_brief,
            commands::library::library_continue,
            commands::library::library_search,
            commands::library::library_search_set,
            commands::instructions::instructions_get,
            commands::instructions::instructions_plan,
            commands::instructions::instructions_apply,
            commands::instructions::instructions_undo,
            commands::instructions::instructions_copy_to_worktrees,
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
            commands::conductor::conductor_start_call,
            commands::conductor::conductor_end_call,
            commands::conductor::conductor_mute,
            commands::conductor::conductor_confirm,
            commands::conductor::conductor_state,
            commands::conductor::conductor_call_log,
            commands::conductor::conductor_call_delete,
            commands::conductor::conductor_ring_set_position,
            commands::conductor::conductor_ring_state,
            commands::auto::auto_start,
            commands::auto::auto_list,
            commands::auto::auto_pause,
            commands::auto::auto_resume,
            commands::auto::auto_stop,
            commands::auto::auto_dismiss,
            commands::phone::phone_pair_begin,
            commands::phone::phone_pair_cancel,
            commands::phone::phone_pairings_list,
            commands::phone::phone_pairing_set_grants,
            commands::phone::phone_pairing_revoke,
            commands::phone::phone_reset,
            commands::phone::phone_arm_approvals,
            commands::phone::phone_arm_state,
            commands::phone::phone_queue_list,
            commands::phone::phone_queue_decide,
            commands::phone::phone_lan_state,
            commands::phone::phone_lan_set,
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
            commands::dictate::dictate_status,
            commands::dictate::dictate_set_binding,
            commands::dictate::dictate_verify_start,
            commands::dictate::dictate_verify_stop,
            commands::dictate::dictate_start_take,
            commands::dictate::dictate_stop_take,
            commands::dictate::dictate_cancel_take,
            commands::dictate::dictate_toggle,
        commands::dictate::dictate_use_local,
        commands::dictate::dictate_quick_action,
            commands::dictate::dictate_composer_focus,
            commands::dictate::dictate_devices,
            commands::dictate::dictate_models,
            commands::dictate::dictate_model_download,
            commands::dictate::dictate_model_cancel,
            commands::dictate::dictate_model_remove,
            commands::dictate::dictate_model_select,
            commands::dictate::dictate_models_remove_unused,
            commands::dictate::dictate_history,
            commands::dictate::dictate_take_delete,
            commands::dictate::dictate_history_clear,
            commands::dictate::dictate_history_export,
            commands::dictate::dictate_take_correct,
            commands::dictate::dictate_stats,
            commands::dictate::dictate_save_file,
            commands::dictate::dictate_pick_file,
            commands::dictate::dictate_share_card_save,
            commands::dictate::dictionary_list,
            commands::dictate::dictionary_save,
            commands::dictate::dictionary_delete,
            commands::dictate::dictionary_restore,
            commands::dictate::dictionary_deleted,
            commands::dictate::dictionary_test,
            commands::dictate::dictionary_import_preview,
            commands::dictate::dictionary_import_commit,
            commands::dictate::dictionary_export,
            commands::dictate::dictionary_suggestions,
            commands::dictate::dictionary_suggestion_resolve,
            commands::dictate::dictionary_sync,
            commands::dictate::dictate_globe_key,
            commands::dictate::dictate_globe_key_set,
            commands::dictate::dictionary_sync_resolve,
            commands::dictate::dictionary_sync_forget,
            commands::dictate::dictate_permissions,
            commands::dictate::dictate_open_permission_settings,
            commands::dictate::dictate_relaunch,
            commands::dictate::dictate_widget_set_position,
            $($extra),*
        ]
    };
}

/// `app_commands!` plus the platform's own: the Simulator's on macOS only
/// (F-CD-21; absent elsewhere, D-1400).
#[cfg(target_os = "macos")]
macro_rules! platform_commands {
    ($($extra:path),* $(,)?) => {
        app_commands![
            commands::simulator::sim_status,
            commands::simulator::sim_boot,
            commands::simulator::sim_shutdown,
            commands::simulator::sim_install,
            commands::simulator::sim_stream_start,
            commands::simulator::sim_stream_stop,
            commands::simulator::sim_screen,
            commands::simulator::sim_tap,
            commands::simulator::sim_text,
            commands::simulator::sim_key,
            commands::simulator::sim_home,
            $($extra),*
        ]
    };
}
#[cfg(not(target_os = "macos"))]
macro_rules! platform_commands {
    ($($extra:path),* $(,)?) => {
        app_commands![$($extra),*]
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
    // The toggle drives Dictate where the user is: focusing Vibezeo would
    // move the insertion target (F-DI-01 "Deep link").
    if link.route == "dictate/toggle" {
        state.dictate.toggle_from_link();
        return;
    }
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
    if link.route == "plugin/callback" {
        // To the attempt waiting for this `state`; a late or unknown one is
        // ignored there (F-PL-03 AC4).
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(link.params.iter())
            .finish();
        let plugins = state.plugins.clone();
        tauri::async_runtime::spawn(async move {
            plugins.deep_link_callback(&query).await;
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
    tauri::async_runtime::spawn(state.audit.clone().run());
    // F-CD-23: language servers stop after ten idle minutes.
    tauri::async_runtime::spawn(crate::services::lsp::run_reaper());
    // F-WT-10: the intent radar follows checkpoints.
    tauri::async_runtime::spawn(crate::services::radar::run(
        state.store.clone(),
        state.data.clone(),
        state.bus.clone(),
        state.terminals.clone(),
    ));
    tauri::async_runtime::spawn(crate::services::hosted::run(
        state.entitlements.clone(),
        state.store.clone(),
        telemetry.clone(),
        state.notifications.clone(),
        state.bus.clone(),
    ));
    let terminals = state.terminals.clone();
    tauri::async_runtime::spawn(async move {
        terminals.login_env().await;
    });
    // Checkpoints past the settings' limits go at startup (F-EX-01).
    let checkpoints = state.checkpoints.clone();
    tauri::async_runtime::spawn(async move {
        checkpoints.prune_all().await;
    });
    // A Best of N check can't outlive the app that ran it (F-EX-04).
    let store = state.store.clone();
    tauri::async_runtime::spawn(async move {
        let _ = tandem_store::repo::bestof::settle_checks(&store).await;
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
    tauri::async_runtime::spawn(supervisor::supervise_task(
        "scheduler/scrollback",
        state.supervision(),
        None,
        move || {
            let (terminals, snapshots) = (terminals.clone(), snapshots.clone());
            async move {
                let mut tick = tokio::time::interval(services::terminal::snapshot_every());
                tick.tick().await;
                loop {
                    tick.tick().await;
                    let (t, dir) = (terminals.clone(), snapshots.clone());
                    // A panicking snapshot fails this job and restarts it.
                    tokio::task::spawn_blocking(move || t.snapshot_all(&dir))
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        },
    ));
    let _ = telemetry;
}

/// Dictate (F-DI-*): events to the main window and the widget, the widget
/// window itself, the tray, then the voice service (mic, hotkeys, model).
fn setup_dictate(app: &tauri::App, state: &AppState) {
    // Conductor shares the Dictate microphone (F-CN-08).
    commands::conductor::install(app.handle());
    use services::dictate::To;
    let emit_app = app.handle().clone();
    state
        .dictate
        .set_emitter(std::sync::Arc::new(move |name, payload, to| {
            let _ = emit_app.emit_to("main", name, payload.clone());
            if to == To::Both {
                let _ = emit_app.emit_to(services::widget::LABEL, name, payload);
            }
        }));
    // Made on the first recording, closed when idle (D-519).
    {
        let widget = std::sync::Arc::new(services::widget::LazyWidget::new(
            std::sync::Arc::new(services::widget::TauriWidget::new(
                app.handle().clone(),
                state.store.clone(),
            )),
            state.clock.clone(),
            services::widget::IDLE_CLOSE,
        ));
        widget.tick_every_30s();
        state.dictate.set_widget(widget);
    }
    tray::install(app.handle());
    // Cloud takes (F-DI-05): the plan check and the STT relay, with a token.
    {
        let (api, auth, ents) = (
            state.api.clone(),
            state.auth.clone(),
            state.entitlements.clone(),
        );
        state.dictate.set_cloud(
            std::sync::Arc::new(move || ents.view().plan),
            std::sync::Arc::new(move |upload| {
                let (api, auth) = (api.clone(), auth.clone());
                Box::pin(async move {
                    auth.with_token(|token| {
                        let (api, upload) = (api.clone(), upload.clone());
                        async move { api.stt_transcribe(&token, upload).await }
                    })
                    .await
                })
            }),
        );
    }
    // Enhance and Polish (F-DI-10): the AI relay's rewrite, with a token.
    {
        let (api, auth) = (state.api.clone(), state.auth.clone());
        state.dictate.set_rewriter(std::sync::Arc::new(
            move |mode, text, instructions, terms| {
                let (api, auth) = (api.clone(), auth.clone());
                Box::pin(async move {
                    auth.with_token(|token| {
                        let (api, text, instructions, terms) = (
                            api.clone(),
                            text.clone(),
                            instructions.clone(),
                            terms.clone(),
                        );
                        async move {
                            api.ai_rewrite(
                                &token,
                                mode.as_str(),
                                &text,
                                instructions.as_deref(),
                                &terms,
                            )
                            .await
                        }
                    })
                    .await
                })
            },
        ));
    }
    // Dictionary sync (F-DI-09): hosted builds, signed in, a plan with it.
    {
        let (api, auth, ents) = (
            state.api.clone(),
            state.auth.clone(),
            state.entitlements.clone(),
        );
        let (api2, auth2, auth3) = (api.clone(), auth.clone(), auth.clone());
        state
            .dictate
            .set_dictionary_sync(services::dictionary_sync::SyncDeps {
                access: std::sync::Arc::new(move || {
                    let a = auth3.state();
                    services::dictionary_sync::SyncAccess {
                        hosted: a.hosted,
                        allowed: ents.view().limits.dictionary_sync,
                        account: a.signed_in.then_some(a.email).flatten(),
                    }
                }),
                call: std::sync::Arc::new(move |req| {
                    let (api, auth) = (api.clone(), auth.clone());
                    Box::pin(async move {
                        auth.with_token(|token| {
                            let (api, req) = (api.clone(), req.clone());
                            async move { api.dictionary_sync(&token, &req).await }
                        })
                        .await
                    })
                }),
                forget: std::sync::Arc::new(move || {
                    let (api, auth) = (api2.clone(), auth2.clone());
                    Box::pin(async move {
                        auth.with_token(|token| {
                            let api = api.clone();
                            async move { api.dictionary_forget(&token).await }
                        })
                        .await
                    })
                }),
            });
        state.dictate.run_sync_loop();
    }
    let dictate = state.dictate.clone();
    // "Hey Conductor" (F-CN-07): the bundled model ships as a resource; dev
    // builds read it from the source tree.
    let model_dir = app
        .path()
        .resource_dir()
        .map(|d| d.join("models/wake"))
        .ok()
        .filter(|d| d.is_dir())
        .unwrap_or_else(tandem_voice::wake::source_model_dir);
    let wake = services::wake::WakeService::new(
        state.store.clone(),
        state.mic.clone(),
        model_dir,
        tandem_voice::wake::process::sidecar(),
    );
    let wake_app = app.handle().clone();
    wake.set_emitter(std::sync::Arc::new(move |s| {
        let _ = wake_app.emit_to("main", "conductor:wake-state", s);
    }));
    wake.set_starter(std::sync::Arc::new(commands::conductor::WakeStart(
        app.handle().clone(),
    )));
    let _ = state.wake.set(wake.clone());
    tauri::async_runtime::spawn(async move {
        dictate.start().await;
        let prefs = dictate.mic_prefs().await;
        wake.start(dictate, prefs).await;
    });
}

/// Builds and runs the Tauri app. Startup follows ARCHITECTURE §1.3: data dir
/// and logging first, then the store, then the hidden main window, which the
/// UI shows on first paint via `app_ready`.
pub fn run() {
    // The OS launcher's headless pass (F-RT-03): no window at all.
    if std::env::args().skip(1).any(|a| a == "--run-routines") {
        std::process::exit(headless::main());
    }
    commands::app::mark_started();
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
            for raw in deeplink::second_instance_links(&args) {
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
    let builder = builder.invoke_handler(platform_commands![
        commands::test::test_counters,
        commands::browser::test_browser_eval,
        commands::test::test_spawn_log,
        commands::test::test_clock_advance,
        commands::test::test_usage_backdate,
        commands::test::test_night_tick,
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
        commands::test::test_dictate_key,
        commands::test::test_dictate_mic,
        commands::test::test_dictate_platform,
        commands::test::test_dictate_widget,
        commands::test::test_plugin_connect,
        commands::test::test_gateway_metrics,
        commands::test::test_task_lane_delay,
        commands::test::test_routines_tick,
        commands::test::test_plugin_price,
        commands::test::test_plugin_allowlist,
        commands::test::test_unattended_run,
        commands::test::test_find_work_server,
        commands::conductor::test_conductor_inject,
        commands::conductor::test_conductor_clock,
        commands::conductor::test_conductor_wake,
        commands::conductor::test_mic_owner,
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(platform_commands![]);

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
            services::events::forward(handle.clone(), &state.bus, state.clock.clone());
            setup_dictate(app, &state);
            ensure_on_screen(app);
            persist_window_state(app);
            spawn_background(&state);
            state.approvals.start();
            // Routines: the scheduler (once it has the app lock), and the
            // plan pausing or resuming them (F-RT-02, F-BL-04).
            state.routines.start(state.data.run(), state.supervision());
            // The tray, while background routines are on (F-SH-13).
            {
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tray::sync(&app, commands::routines::background_on().await).await;
                });
            }
            // Tasks refresh when anyone writes them (F-TK-03).
            state.tasks.watch(state.bus.clone());
            // Task runs: those the last run left are stopped; signing out
            // or losing Pro stops the active ones (F-TK-04, UI-84).
            {
                let exec = state.task_exec.clone();
                let (auth, ent) = (state.auth.clone(), state.entitlements.clone());
                let mut app_events = state.bus.subscribe(tandem_core::bus::Topic::App);
                let mut plan_events = state.bus.subscribe(tandem_core::bus::Topic::Entitlement);
                let routines = state.routines.clone();
                let (restore, threads) = (state.restore.clone(), state.threads.clone());
                tauri::async_runtime::spawn(async move {
                    // Threads and attempts a headless pass is still running
                    // aren't stale: settle them only once this app holds the
                    // lock (F-RT-06, D-2540). The panes paused at launch.
                    routines.wait_lock().await;
                    if restore.settle_threads().await > 0 {
                        threads.republish().await;
                    }
                    exec.settle().await;
                    loop {
                        use tokio::sync::broadcast::error::RecvError;
                        let (app, plan) = tokio::select! {
                            e = app_events.recv() => (Some(e), None),
                            e = plan_events.recv() => (None, Some(e)),
                        };
                        if matches!(app, Some(Err(RecvError::Closed))) || matches!(plan, Some(Err(RecvError::Closed))) {
                            break;
                        }
                        if let Some(Ok(e)) = &app {
                            if e.name == "auth:changed" && !auth.is_signed_in() {
                                exec.stop_all("you signed out").await;
                            }
                        }
                        if plan.is_some() && !ent.limits().task_execution {
                            exec.stop_all("plan changed").await;
                        }
                    }
                });
            }
            {
                let (routines, ent) = (state.routines.clone(), state.entitlements.clone());
                let mut events = state.bus.subscribe(tandem_core::bus::Topic::Entitlement);
                let ring_app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    routines.apply_plan(&ent.limits()).await;
                    loop {
                        use tokio::sync::broadcast::error::RecvError;
                        match events.recv().await {
                            Ok(_) | Err(RecvError::Lagged(_)) => {
                                routines.apply_plan(&ent.limits()).await;
                                // The floating ring needs Conductor on the plan (F-CN-04).
                                commands::conductor::sync_ring(&ring_app).await;
                            }
                            Err(RecvError::Closed) => break,
                        }
                    }
                });
            }
            // One gateway for every session, up from launch (F-PL-01).
            {
                let gateway = state.gateway.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = gateway.handle().await {
                        tracing::warn!(code = "gateway.start_failed", error = %e.message, "the MCP gateway didn't start");
                    }
                });
            }
            // Mail queued before the app last quit goes out (F-AG-12).
            state.mail.pump_soon();
            commands::phone::start(&handle);
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
                // The runaway guard watches agents from here on (F-AC-09).
                state.runaway.start(handle.clone());
                // The Night Shift starts due runs and sends its summary (F-RT-06).
                let night = handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let state = night.state::<AppState>();
                        // A headless pass owns the runs until it ends (F-RT-06).
                        if state.routines.holds_lock() {
                            state.night_shift.tick(&state).await;
                            // Summaries a headless pass wrote, or this tick.
                            for alert in state.night_shift.take_alerts(state.clock.now_ms()) {
                                state.phone.queue_alert(alert);
                            }
                        }
                        state.clock.sleep(crate::services::night_shift::TICK).await;
                    }
                });
                // F-DB-04: the review queue follows the status bus.
                let _ = state
                    .review
                    .set(services::review_queue::start(handle.clone()));
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
            // SSH hosts (F-RM-01): the bundled helper binaries and the
            // askpass helper's loopback endpoint.
            {
                let state = handle.state::<AppState>();
                state
                    .remote
                    .set_bin_dir(services::remote::bin_dir(handle.path().resource_dir().ok()));
                let remote_state = handle.clone();
                tauri::async_runtime::spawn(async move {
                    match services::askpass::AskpassServer::start().await {
                        Ok(server) => remote_state.state::<AppState>().remote.set_askpass(server),
                        Err(err) => tracing::warn!(code = "remote.askpass_failed", error = %err, "SSH password prompts are off"),
                    }
                });
            }
            // Threads on the Dashboard from the start (D-6).
            let threads_state = handle.clone();
            tauri::async_runtime::spawn(async move {
                threads_state.state::<AppState>().threads.announce().await;
            });
            // Remote config (model catalog, registry overrides) has a 10-min
            // TTL: a model added remotely shows up without an update (F-TH-08 AC1).
            let config_state = handle.clone();
            tauri::async_runtime::spawn(supervisor::supervise_task(
                "scheduler/config",
                state.supervision(),
                None,
                move || {
                    let config_state = config_state.clone();
                    async move {
                        let mut tick = tokio::time::interval(services::config::CONFIG_TTL);
                        tick.tick().await;
                        loop {
                            tick.tick().await;
                            let state = config_state.state::<AppState>();
                            let config = state.config.refresh().await;
                            state.terminals.engines.apply_config(&config);
                        }
                    }
                },
            ));
            // Worktree sweep: at startup, then every 6 h (F-CD-15). It never
            // deletes anything; finished worktrees become one notification.
            let sweep_state = handle.clone();
            tauri::async_runtime::spawn(supervisor::supervise_task(
                "scheduler/worktree-sweep",
                state.supervision(),
                None,
                move || {
                    let sweep_state = sweep_state.clone();
                    async move {
                        let mut tick = tokio::time::interval(services::lanes::SWEEP_EVERY);
                        loop {
                            tick.tick().await;
                            let state = sweep_state.state::<AppState>();
                            if let Err(err) = services::lanes::sweep_and_notify(&state).await {
                                tracing::info!(error = %err, "worktree sweep skipped");
                            }
                        }
                    }
                },
            ));
            // Detect again when the window comes back, at most once a minute.
            if let Some(window) = app.get_webview_window("main") {
                let focus = handle.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(focused) = event {
                        let state = focus.state::<AppState>();
                        state.notifications.set_focused(*focused);
                        // A composer counts as the Dictate target only while
                        // the main window has focus (F-DI-12).
                        state.dictate.set_main_focused(*focused);
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
            // Compiled only with debug assertions: `e2e` is always false in a
            // release build, but the capability's text would still ship in
            // the binary (scripts/check-release-harness.sh, D-1011).
            #[cfg(debug_assertions)]
            if e2e {
                app.add_capability(include_str!("../test-capability.json"))?;
            }
            // The Simulator's permissions exist only in macOS builds (build.rs).
            #[cfg(target_os = "macos")]
            app.add_capability(include_str!("../sim-capability.json"))?;
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
                state.remote.shutdown();
                // F-WT-07: a landing's check doesn't outlive the app.
                services::merge_queue::shutdown();
                // F-CD-23: language servers get shutdown and exit.
                tauri::async_runtime::block_on(async {
                    let stop = services::lsp::global().shutdown();
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), stop).await;
                });
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
            lib.contains("#[cfg(debug_assertions)]\n            if e2e {\n                app.add_capability"),
            "the harness capability is compiled only with debug assertions, and added only with e2e"
        );
        let commands = include_str!("commands/mod.rs");
        assert!(
            commands.contains("#[cfg(debug_assertions)]\npub mod test;"),
            "the harness module is debug-only"
        );
    }

    /// F-TL-04: Linux releases come as a .deb, an .rpm and an AppImage from
    /// both release paths, and each is checked for tandem-host (D-832).
    #[test]
    fn f_tl_04_linux_releases_build_deb_rpm_and_appimage() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let targets: Vec<&str> = conf["bundle"]["targets"]
            .as_array()
            .expect("bundle.targets")
            .iter()
            .filter_map(|t| t.as_str())
            .collect();
        for t in ["appimage", "deb", "rpm"] {
            assert!(targets.contains(&t), "{t} in {targets:?}");
        }
        let local = include_str!("../../../../scripts/release-local.sh");
        assert!(local.contains("--bundles appimage,deb,rpm"));
        assert!(local.contains(r#""$bundle"/rpm/*-"$version"-*.rpm"#));
        let workflow = include_str!("../../../../.github/workflows/release.yml");
        assert!(workflow.contains(r#"bundles: "appimage,deb,rpm""#));
        assert!(workflow.contains("bundle/rpm/*.rpm"));
        assert!(include_str!("../../../../scripts/check-host-bundle.sh").contains("*.rpm)"));
    }

    /// macOS bundle (D-2101, NFR macOS): macOS 14 or later as DECISIONS
    /// says, the hardened runtime with only the microphone entitlement, no
    /// App Sandbox (D-2106), and the microphone's usage string, without which
    /// the first take would end the app. Tauri reads these files only when
    /// building for macOS, so this test is what keeps them valid elsewhere.
    #[test]
    fn nfr_macos_bundle_config_is_pinned() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.macos.conf.json"))
                .expect("tauri.macos.conf.json");
        let mac = &conf["bundle"]["macOS"];
        // Tauri's own schema accepts it (unknown keys are an error there).
        let parsed: tauri::utils::config::MacConfig =
            serde_json::from_value(mac.clone()).expect("a valid bundle.macOS");
        assert_eq!(parsed.minimum_system_version.as_deref(), Some("14.0"));
        assert!(parsed.hardened_runtime);
        assert_eq!(parsed.entitlements.as_deref(), Some("./Entitlements.plist"));
        assert!(
            parsed.signing_identity.is_none(),
            "the identity comes from CI (APPLE_SIGNING_IDENTITY), never the repo"
        );
        assert_eq!(conf["app"]["windows"][0]["title"], "Vibezeo");

        let ent = plist::Value::from_file(root.join("Entitlements.plist")).expect("entitlements");
        let ent = ent.as_dictionary().expect("dict");
        let keys: Vec<&str> = ent.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["com.apple.security.device.audio-input"],
            "each entitlement is justified in Entitlements.plist; add it there and here"
        );
        assert_eq!(
            ent["com.apple.security.device.audio-input"].as_boolean(),
            Some(true)
        );

        let info = plist::Value::from_file(root.join("Info.plist")).expect("Info.plist");
        let info = info.as_dictionary().expect("dict");
        let mic = info
            .get("NSMicrophoneUsageDescription")
            .and_then(plist::Value::as_string)
            .expect("NSMicrophoneUsageDescription");
        assert!(mic.contains("Vibezeo") && mic.contains("Dictate"), "{mic}");
        for owned_by_tauri in [
            "CFBundleIdentifier",
            "LSMinimumSystemVersion",
            "CFBundleURLTypes",
        ] {
            assert!(
                !info.contains_key(owned_by_tauri),
                "{owned_by_tauri} comes from the config"
            );
        }
    }

    /// macOS release (D-2102): notarization rejects an unsigned Mach-O
    /// anywhere in the bundle, and Tauri doesn't sign resources. Both macOS
    /// release paths sign every sidecar in `externalBin` and the bundled
    /// tandem-host before bundling, notarize the dmg and verify the app.
    #[test]
    fn nfr_macos_release_signs_every_bundled_executable() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo = root.join("../../..");
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.sidecars.conf.json")).expect("json");
        let sidecars: Vec<String> = conf["bundle"]["externalBin"]
            .as_array()
            .expect("externalBin")
            .iter()
            .filter_map(|b| b.as_str())
            .map(str::to_string)
            .collect();
        assert!(sidecars.len() >= 2, "{sidecars:?}");
        let workflow = std::fs::read_to_string(repo.join(".github/workflows/release.yml"))
            .expect("release.yml");
        let local =
            std::fs::read_to_string(repo.join("scripts/ci/macos-release.sh")).expect("script");
        for (name, text) in [("release.yml", &workflow), ("macos-release.sh", &local)] {
            for bin in &sidecars {
                // `binaries/tandem-tasks` → signed as `binaries/tandem-tasks-<target>`.
                assert!(text.contains(&format!("{bin}-")), "{name} signs {bin}");
            }
            assert!(
                text.contains("tandem-host/macos-universal/tandem-host"),
                "{name} signs the macOS tandem-host"
            );
            for step in [" identity)", " notarize ", "verify --notarized"] {
                assert!(text.contains(step), "{name}: {step}");
            }
        }
        // The helper is signed where it's built, before its SHA-256 manifest.
        let host_job = workflow
            .split("\n  build:\n")
            .next()
            .expect("jobs before build");
        assert!(host_job.contains(
            "sign \"$id\" apps/desktop/src-tauri/resources/tandem-host/macos-universal/tandem-host"
        ));
        // The release builds the UI it embeds (no beforeBuildCommand).
        assert!(workflow.contains("pnpm --filter desktop build"));
    }

    /// §5.3: the app webview's CSP is `'self'` only, runs no remote or inline
    /// script, and connects only to itself, the IPC bridge, loopback (the
    /// gateway and local review) and the API with its WebSocket.
    #[test]
    fn nfr_5_3_csp_is_self_only_and_connects_to_the_listed_hosts() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        assert_eq!(conf["app"]["withGlobalTauri"], false);
        let csp = &conf["app"]["security"]["csp"];
        let dir = |k: &str| csp[k].as_str().unwrap_or_default().to_string();
        assert_eq!(dir("default-src"), "'self'");
        assert_eq!(
            dir("script-src"),
            "'self'",
            "no remote, inline or eval script"
        );
        assert_eq!(dir("object-src"), "'none'");
        assert_eq!(dir("base-uri"), "'self'");
        assert_eq!(dir("frame-ancestors"), "'none'");
        for k in ["img-src", "font-src", "style-src"] {
            assert!(!dir(k).contains("http"), "{k} loads nothing remote");
        }
        let allowed = |src: &str| {
            matches!(
                src,
                "'self'"
                    | "ipc:"
                    | "http://ipc.localhost"
                    | "http://127.0.0.1:*"
                    | "ws://127.0.0.1:*"
                    | "http://localhost:*"
                    | "http://api.tandem.local:8787"
                    | "https://api.vibezeo.com"
                    | "wss://api.vibezeo.com"
            )
        };
        let connect = dir("connect-src");
        assert!(!connect.is_empty());
        for src in connect.split_whitespace() {
            assert!(allowed(src), "connect-src {src} is not an allowed host");
        }
    }

    /// §5.3 least privilege: each capability names its app window (the main
    /// window, and the Dictate widget with only what it needs); none names a
    /// webview, and the in-app browser's pages are plain wry webviews with no
    /// IPC handler, so they can call no command (E2E-M9-3 probes `__TAURI__`).
    #[test]
    fn nfr_5_3_only_app_windows_have_commands_and_browser_pages_have_no_ipc() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("capabilities dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(
            files,
            vec![
                "main.json".to_string(),
                "ring.json".to_string(),
                "widget.json".to_string()
            ]
        );
        for (file, window) in [
            ("main.json", "main"),
            ("ring.json", "conductor-ring"),
            ("widget.json", "dictate-widget"),
        ] {
            let cap: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join(file)).expect("read"))
                    .expect("json");
            assert_eq!(cap["windows"], serde_json::json!([window]), "{file}");
            assert!(cap.get("webviews").is_none(), "{file} names a webview");
        }
        // Conductor's floating ring gets exactly this (D-1117): start, end
        // and mute a call, its state and position, the call log through an
        // event to the main window. Never `conductor_confirm`: a click there
        // can't confirm anything (F-CN-03).
        let ring: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/ring.json")).expect("ring.json");
        assert_eq!(
            ring["permissions"],
            serde_json::json!([
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "core:event:allow-emit-to",
                "core:window:allow-start-dragging",
                "core:window:allow-outer-position",
                "allow-conductor-start-call",
                "allow-conductor-end-call",
                "allow-conductor-mute",
                "allow-conductor-ring-state",
                "allow-conductor-ring-set-position",
                "allow-app-log"
            ])
        );
        // The widget gets exactly this; anything more is a reviewed change.
        // Reviewed (D-825): emit-to carries See plans to the main window,
        // use-local is "Use local for this take" after a cloud failure
        // (F-DI-05), and quick-action is Enhance and Polish (F-DI-10).
        let widget: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/widget.json")).expect("widget.json");
        assert_eq!(
            widget["permissions"],
            serde_json::json!([
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "core:event:allow-emit-to",
                "core:window:allow-start-dragging",
                "allow-dictate-status",
                "allow-dictate-cancel-take",
                "allow-dictate-use-local",
                "allow-dictate-quick-action",
                "allow-dictate-widget-set-position",
                "allow-app-log"
            ])
        );
        let host = include_str!("browser/wry_host.rs");
        for banned in [
            "with_ipc_handler",
            "WebviewBuilder",
            "WebviewWindowBuilder",
            "add_child",
        ] {
            assert!(
                !host.contains(banned),
                "browser pages must not use {banned}"
            );
        }
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
        let sim = include_str!("../sim-capability.json");
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
            } else if name.starts_with("sim_") {
                sim
            } else {
                main
            };
            assert!(
                granted.contains(&allow),
                "{allow} is missing from the capability"
            );
        }
    }

    /// F-CD-21 AC2 off macOS: the Simulator's commands are registered only in
    /// the macOS macro, their permissions are generated only for macOS
    /// targets, the shipped capability grants none, and the module isn't in
    /// a non-macOS, non-test build (D-1400).
    #[test]
    fn f_cd_21_2_the_simulator_exists_only_on_macos() {
        let lib = include_str!("lib.rs");
        let mac_macro = lib
            .split("#[cfg(target_os = \"macos\")]\nmacro_rules! platform_commands")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(not(target_os = \"macos\"))]").next())
            .expect("the macOS command list");
        let everywhere = lib
            .split("macro_rules! platform_commands")
            .next()
            .unwrap_or("");
        assert!(mac_macro.contains("commands::simulator::sim_status,"));
        assert!(
            !everywhere.contains("commands::simulator::"),
            "a simulator command outside the macOS list"
        );
        let build = include_str!("../build.rs");
        assert!(build
            .contains("if target_os == \"macos\" {\n        all.extend_from_slice(SIM_COMMANDS);"));
        let commands_list = build.split("const SIM_COMMANDS").next().unwrap_or("");
        assert!(
            !commands_list.contains("\"sim_"),
            "a simulator command in COMMANDS"
        );
        let main = include_str!("../capabilities/main.json");
        assert!(
            !main.contains("allow-sim-"),
            "the shipped capability grants a simulator command"
        );
        let modules = include_str!("commands/mod.rs");
        assert!(modules.contains("#[cfg(any(target_os = \"macos\", test))]\npub mod simulator;"));
        if !cfg!(target_os = "macos") {
            assert!(crate::services::simulator::host().is_none());
        }
    }
}
