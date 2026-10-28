use std::collections::BTreeMap;

use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use tandem_core::bus::{BusEvent, Topic};
use tandem_core::entitlement::Limits;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::settings;

use crate::state::AppState;

#[tauri::command]
pub async fn settings_get_all(
    state: State<'_, AppState>,
) -> Result<BTreeMap<SettingKey, Value>, IpcError> {
    Ok(settings::get_all(&state.store).await?)
}

/// Stores and applies a setting immediately (F-SH-04).
#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    key: SettingKey,
    value: Value,
) -> Result<(), IpcError> {
    store_and_apply(&app, &state, key, value).await
}

/// `settings_set`'s work, for callers in Rust (Conductor's `set_setting`).
pub async fn store_and_apply(
    app: &AppHandle,
    state: &AppState,
    key: SettingKey,
    value: Value,
) -> Result<(), IpcError> {
    store(state, key, &value).await?;
    apply(app, state, key, &value).await?;
    // F-CD-23 (D-2591): with the sandbox on, worktrees need their own
    // trust, so servers started under the repository's stop and start again
    // only past the new check.
    if key == SettingKey::AgentSandbox {
        crate::services::lsp::global().shutdown().await;
    }
    state.bus.publish(BusEvent::new(
        Topic::App,
        "settings:changed",
        serde_json::json!({ "key": key, "value": value }),
    ));
    Ok(())
}

#[tauri::command]
pub async fn settings_reset(
    app: AppHandle,
    state: State<'_, AppState>,
    key: SettingKey,
) -> Result<(), IpcError> {
    let value = reset(&state, key).await?;
    apply(&app, &state, key, &value).await?;
    state.bus.publish(BusEvent::new(
        Topic::App,
        "settings:changed",
        serde_json::json!({ "key": key, "value": value }),
    ));
    Ok(())
}

/// What the limits allow a setting to be: session search needs
/// `session_search` (on with no account, D-2900), whoever turns
/// it on (F-TH-17, D-2414).
fn allowed(limits: &Limits, key: SettingKey, value: &Value) -> Result<(), IpcError> {
    if key == SettingKey::SessionSearch && value.as_bool() == Some(true) && !limits.session_search {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Searching session text is turned off for this account.",
        ));
    }
    Ok(())
}

/// Stores a setting and runs the effects that need no window: every path
/// that sets one (`settings_set`, Conductor's `set_setting`) comes here.
async fn store(state: &AppState, key: SettingKey, value: &Value) -> Result<(), IpcError> {
    allowed(&state.entitlements.limits(), key, value)?;
    // The sandbox turns on only where it can run, so nobody believes
    // they're sandboxed when they aren't (F-TL-11).
    if key == SettingKey::AgentSandbox {
        crate::commands::sandbox::may_turn_on(state.terminals.sandboxer().await.as_ref(), value)?;
    }
    // A memory cap is offered only where it applies (F-EX-06).
    if key == SettingKey::AgentMemoryCap {
        crate::commands::resources::may_cap(&state.terminals.resources.status().await, value)?;
    }
    settings::set(&state.store, key, value.clone()).await?;
    stored(state, key, value);
    Ok(())
}

/// Resets a setting to its default, with the same effects; the new value.
async fn reset(state: &AppState, key: SettingKey) -> Result<Value, IpcError> {
    settings::reset(&state.store, Some(vec![key])).await?;
    let value = settings::get(&state.store, key).await?;
    stored(state, key, &value);
    Ok(value)
}

fn stored(state: &AppState, key: SettingKey, value: &Value) {
    // Off deletes the session index, however it was turned off (F-TH-17).
    if key == SettingKey::SessionSearch && value.as_bool() != Some(true) {
        crate::services::library::delete_index(state.data.root());
    }
    // Checks started from now on take turns, or run at once (F-EX-06).
    if key == SettingKey::ChecksOneAtATime {
        crate::services::check_queue::set(value.as_bool().unwrap_or(false));
    }
}

/// Side effects that live outside the webview.
async fn apply(
    app: &AppHandle,
    state: &AppState,
    key: SettingKey,
    value: &Value,
) -> Result<(), IpcError> {
    if key.as_str().starts_with("dictate_") || key == SettingKey::Theme {
        state.dictate.reload_settings(key).await;
    }
    // The wake listener follows its own settings and the Dictate mic (F-CN-07).
    if key.as_str().starts_with("conductor_wake")
        || matches!(key, SettingKey::DictateMic | SettingKey::DictateBackupMic)
    {
        if let Some(w) = state.wake.get() {
            w.reload(state.dictate.mic_prefs().await).await;
        }
    }
    match key {
        SettingKey::TelemetryUsage | SettingKey::TelemetryCrash => state.apply_settings().await,
        SettingKey::ExposeAllPluginTools => {
            state
                .gateway
                .set_expose_all(value.as_bool().unwrap_or(false));
        }
        // Paused: queued mail waits; resumed: it goes out, once (F-AG-12).
        SettingKey::PauseAllMail => state.mail.pump_soon(),
        // The floating ring shows or hides (F-CN-04).
        SettingKey::ConductorFloatingRing => crate::commands::conductor::sync_ring(app).await,
        // The tray's check mark follows; rows re-read (F-SH-13).
        SettingKey::PauseAllRoutines => {
            state.routines.notify_changed();
            crate::tray::sync(app, crate::commands::routines::background_on().await).await;
        }
        SettingKey::LaunchAtLogin => {
            let on = value.as_bool().unwrap_or(false);
            // E2E runs never touch the real login items.
            if !state.e2e {
                let launcher = app.autolaunch();
                let result = if on {
                    launcher.enable()
                } else {
                    launcher.disable()
                };
                result.map_err(|e| {
                    IpcError::new(
                        ErrorCode::Unavailable,
                        format!("Couldn't change launch at login: {e}"),
                    )
                })?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::library::INDEX_FILE;
    use tandem_core::DataDir;
    use tandem_store::Store;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, AppState::for_tests(data, store))
    }

    fn index(state: &AppState) -> std::path::PathBuf {
        let path = state.data.root().join(INDEX_FILE);
        std::fs::write(&path, b"index").expect("index");
        path
    }

    #[tokio::test]
    async fn f_th_17_4_turning_search_off_through_settings_set_deletes_the_index() {
        let (_dir, state) = state();
        let path = index(&state);
        store(&state, SettingKey::SessionSearch, &Value::Bool(false))
            .await
            .expect("set");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn f_th_17_4_resetting_search_deletes_the_index() {
        let (_dir, state) = state();
        let path = index(&state);
        let value = reset(&state, SettingKey::SessionSearch)
            .await
            .expect("reset");
        assert_eq!(value, Value::Bool(false));
        assert!(!path.exists());
        // Other settings leave it alone.
        let path = index(&state);
        reset(&state, SettingKey::Theme).await.expect("reset");
        assert!(path.exists());
    }

    #[tokio::test]
    async fn f_th_17_4_search_turns_on_with_no_account_and_limits_can_refuse_it() {
        let (_dir, state) = state();
        // No account: the limits carry session search (D-869, D-2900).
        store(&state, SettingKey::SessionSearch, &Value::Bool(true))
            .await
            .expect("on with no account");
        assert_eq!(
            settings::get(&state.store, SettingKey::SessionSearch)
                .await
                .expect("get"),
            Value::Bool(true)
        );
        // Limits that turn it off refuse turning it on, never turning it off.
        let off = Limits {
            session_search: false,
            ..Limits::free()
        };
        let err = allowed(&off, SettingKey::SessionSearch, &Value::Bool(true)).expect_err("off");
        assert_eq!(err.code, ErrorCode::ForbiddenPlan);
        assert!(allowed(&off, SettingKey::SessionSearch, &Value::Bool(false)).is_ok());
    }
}
