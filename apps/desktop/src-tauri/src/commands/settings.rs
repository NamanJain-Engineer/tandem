use std::collections::BTreeMap;

use serde_json::Value;
use tauri::State;

use tandem_core::bus::{BusEvent, Topic};
use tandem_core::settings::SettingKey;
use tandem_core::IpcError;
use tandem_store::repo::settings;

use crate::state::AppState;

#[tauri::command]
pub async fn settings_get_all(
    state: State<'_, AppState>,
) -> Result<BTreeMap<SettingKey, Value>, IpcError> {
    Ok(settings::get_all(&state.store).await?)
}

#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: SettingKey,
    value: Value,
) -> Result<(), IpcError> {
    settings::set(&state.store, key, value.clone()).await?;
    state.bus.publish(BusEvent::new(
        Topic::App,
        "settings:changed",
        serde_json::json!({ "key": key, "value": value }),
    ));
    Ok(())
}

#[tauri::command]
pub async fn settings_reset(state: State<'_, AppState>, key: SettingKey) -> Result<(), IpcError> {
    settings::reset(&state.store, Some(vec![key])).await?;
    Ok(())
}
