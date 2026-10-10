use std::collections::BTreeMap;

use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use tandem_core::bus::{BusEvent, Topic};
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
    settings::set(&state.store, key, value.clone()).await?;
    apply(&app, &state, key, &value).await?;
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
    settings::reset(&state.store, Some(vec![key])).await?;
    let value = settings::get(&state.store, key).await?;
    apply(&app, &state, key, &value).await?;
    state.bus.publish(BusEvent::new(
        Topic::App,
        "settings:changed",
        serde_json::json!({ "key": key, "value": value }),
    ));
    Ok(())
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
