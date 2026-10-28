//! Dictate commands (ARCHITECTURE §3.2 "Dictate"): thin wrappers over
//! `services::dictate`.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

use tandem_core::{ErrorCode, IpcError};
use tandem_voice::devices::DeviceInfo;
use tandem_voice::dictionary::Rule;
use tandem_voice::hotkey::HotkeyStatus;
use tandem_voice::permissions::{grant_step, Access, GrantStep, Privacy, PrivacyProbe};
use tandem_voice::take::Origin;

use crate::services::dictate::{
    DictateStats, DictateStatus, HistoryCursor, ImportPreview, ModelsView, Suggestion, TakePage,
};
use crate::state::AppState;

#[tauri::command]
pub async fn dictate_status(state: State<'_, AppState>) -> Result<DictateStatus, IpcError> {
    Ok(state.dictate.status().await)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum BindingMode {
    Hold,
    Toggle,
}

#[tauri::command]
pub async fn dictate_set_binding(
    state: State<'_, AppState>,
    binding: Option<String>,
    mode: BindingMode,
) -> Result<HotkeyStatus, IpcError> {
    state
        .dictate
        .set_binding(binding, mode == BindingMode::Toggle)
        .await
}

#[tauri::command]
pub fn dictate_verify_start(state: State<'_, AppState>) -> Result<(), IpcError> {
    if let Some(c) = state.dictate.controller() {
        c.verify_start();
    }
    Ok(())
}

#[tauri::command]
pub fn dictate_verify_stop(state: State<'_, AppState>) -> Result<(), IpcError> {
    if let Some(c) = state.dictate.controller() {
        c.verify_stop();
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StartedTake {
    pub take_id: String,
}

#[tauri::command]
pub fn dictate_start_take(
    state: State<'_, AppState>,
    origin: Origin,
) -> Result<StartedTake, IpcError> {
    let id = state.dictate.start_take(origin)?;
    Ok(StartedTake {
        take_id: id.as_str().to_string(),
    })
}

#[tauri::command]
pub fn dictate_stop_take(state: State<'_, AppState>) -> Result<(), IpcError> {
    if let Some(c) = state.dictate.controller() {
        c.stop_take();
    }
    Ok(())
}

#[tauri::command]
pub fn dictate_cancel_take(state: State<'_, AppState>) -> Result<(), IpcError> {
    if let Some(c) = state.dictate.controller() {
        c.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn dictate_toggle(state: State<'_, AppState>) -> Result<(), IpcError> {
    if let Some(c) = state.dictate.controller() {
        c.toggle();
    }
    Ok(())
}

/// "Enhance" / "Polish" after a take (F-DI-10): false when there's no take
/// to rewrite or one is running. `disclosed`: the user just read the
/// one-time disclosure in the widget, which is recorded first.
#[tauri::command]
pub async fn dictate_quick_action(
    state: State<'_, AppState>,
    mode: tandem_voice::cloud::RewriteMode,
    disclosed: Option<bool>,
) -> Result<bool, IpcError> {
    if disclosed == Some(true) {
        state.dictate.mark_enhance_disclosed().await?;
    }
    Ok(state.dictate.quick_action(mode))
}

/// "Use local for this take" (F-DI-05): the last failed cloud take,
/// transcribed on this device. False when there's nothing to retry.
#[tauri::command]
pub fn dictate_use_local(state: State<'_, AppState>) -> bool {
    state.dictate.use_local()
}

#[tauri::command]
pub fn dictate_composer_focus(
    state: State<'_, AppState>,
    target_id: Option<String>,
) -> Result<(), IpcError> {
    state.dictate.set_composer_focus(target_id);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DevicesView {
    pub devices: Vec<DeviceInfo>,
    /// What the device policy picks right now (F-DI-03).
    pub chosen: Option<String>,
}

#[tauri::command]
pub async fn dictate_devices(state: State<'_, AppState>) -> Result<DevicesView, IpcError> {
    let devices = state.dictate.devices();
    let status = state.dictate.status().await;
    Ok(DevicesView {
        chosen: status.mic.map(|m| m.id),
        devices,
    })
}

#[tauri::command]
pub fn dictate_models(state: State<'_, AppState>) -> Result<ModelsView, IpcError> {
    Ok(state.dictate.models_view())
}

#[tauri::command]
pub fn dictate_model_download(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<ModelsView, IpcError> {
    state
        .dictate
        .download(&model_id, state.net.client().clone())
}

#[tauri::command]
pub fn dictate_model_cancel(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<ModelsView, IpcError> {
    Ok(state.dictate.cancel_download(&model_id))
}

#[tauri::command]
pub async fn dictate_model_remove(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<ModelsView, IpcError> {
    state.dictate.remove_model(&model_id).await
}

#[tauri::command]
pub async fn dictate_model_select(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<ModelsView, IpcError> {
    state.dictate.select_model(&model_id).await
}

#[tauri::command]
pub async fn dictate_models_remove_unused(
    state: State<'_, AppState>,
) -> Result<ModelsView, IpcError> {
    state.dictate.remove_unused().await
}

#[tauri::command]
pub async fn dictate_history(
    state: State<'_, AppState>,
    query: Option<String>,
    before: Option<HistoryCursor>,
    limit: Option<u32>,
) -> Result<TakePage, IpcError> {
    state.dictate.history(query, before, limit).await
}

#[tauri::command]
pub async fn dictate_take_delete(
    state: State<'_, AppState>,
    take_id: String,
) -> Result<(), IpcError> {
    tandem_store::repo::dictate::delete_take(state.dictate.store(), take_id).await?;
    Ok(())
}

#[tauri::command]
pub async fn dictate_history_clear(state: State<'_, AppState>) -> Result<(), IpcError> {
    tandem_store::repo::dictate::clear_history(state.dictate.store()).await?;
    Ok(())
}

#[tauri::command]
pub async fn dictate_history_export(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), IpcError> {
    state.dictate.export_history(&path).await
}

#[tauri::command]
pub async fn dictate_take_correct(
    state: State<'_, AppState>,
    take_id: String,
    corrected: String,
    remember: bool,
) -> Result<Option<Rule>, IpcError> {
    state
        .dictate
        .correct_take(&take_id, &corrected, remember)
        .await
}

#[tauri::command]
pub async fn dictate_stats(state: State<'_, AppState>) -> Result<DictateStats, IpcError> {
    state.dictate.stats().await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "DictateFileKind")]
pub enum FileKind {
    Csv,
    Png,
    Json,
}

impl FileKind {
    fn ext(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Png => "png",
            Self::Json => "json",
        }
    }
}

/// The harness's stand-in for a file dialog: `TANDEM_FAKE_SAVE_DIR` (debug).
fn fake_dir() -> Option<std::path::PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("TANDEM_FAKE_SAVE_DIR").map(std::path::PathBuf::from)
}

#[tauri::command]
pub async fn dictate_save_file(
    app: AppHandle,
    suggested_name: String,
    kind: FileKind,
) -> Result<Option<String>, IpcError> {
    let name: String = suggested_name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        .take(80)
        .collect();
    if let Some(dir) = fake_dir() {
        return Ok(Some(dir.join(&name).to_string_lossy().into_owned()));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&name)
        .add_filter(kind.ext(), &[kind.ext()])
        .save_file(move |picked| {
            let _ = tx.send(
                picked
                    .and_then(|p| p.into_path().ok())
                    .map(|p| p.to_string_lossy().into_owned()),
            );
        });
    Ok(rx.await.ok().flatten())
}

#[tauri::command]
pub async fn dictate_pick_file(app: AppHandle, kind: FileKind) -> Result<Option<String>, IpcError> {
    if let Some(dir) = fake_dir() {
        let path = dir.join(format!("import.{}", kind.ext()));
        return Ok(path.exists().then(|| path.to_string_lossy().into_owned()));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter(kind.ext(), &[kind.ext()])
        .pick_file(move |picked| {
            let _ = tx.send(
                picked
                    .and_then(|p| p.into_path().ok())
                    .map(|p| p.to_string_lossy().into_owned()),
            );
        });
    Ok(rx.await.ok().flatten())
}

#[tauri::command]
pub fn dictate_share_card_save(path: String, png_base64: String) -> Result<(), IpcError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.trim_start_matches("data:image/png;base64,"))
        .map_err(|_| IpcError::new(ErrorCode::Invalid, "That isn't a PNG image"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.len() > 20 * 1024 * 1024 {
        return Err(IpcError::new(ErrorCode::Invalid, "That isn't a PNG image"));
    }
    tandem_core::atomic::atomic_write(std::path::Path::new(&path), &bytes, None)
        .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't save the image: {e}")))
}

#[tauri::command]
pub fn dictionary_list(state: State<'_, AppState>) -> Result<Vec<Rule>, IpcError> {
    Ok(state.dictate.rules())
}

#[tauri::command]
pub async fn dictionary_save(state: State<'_, AppState>, rule: Rule) -> Result<Rule, IpcError> {
    state.dictate.save_rule(rule).await
}

#[tauri::command]
pub async fn dictionary_delete(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), IpcError> {
    state.dictate.delete_rules(ids).await
}

#[tauri::command]
pub async fn dictionary_restore(state: State<'_, AppState>, id: String) -> Result<Rule, IpcError> {
    state.dictate.restore_rule(&id).await
}

#[tauri::command]
pub async fn dictionary_deleted(state: State<'_, AppState>) -> Result<Vec<Rule>, IpcError> {
    state.dictate.deleted_rules().await
}

#[tauri::command]
pub fn dictionary_test(
    state: State<'_, AppState>,
    sample: String,
    app_id: Option<String>,
    lang: Option<String>,
) -> Result<String, IpcError> {
    if sample.len() > 10_000 {
        return Err(IpcError::new(ErrorCode::Invalid, "Try a shorter sample"));
    }
    Ok(state
        .dictate
        .test_dictionary(&sample, app_id.as_deref(), lang.as_deref()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RulesFormat {
    Json,
    Csv,
}

#[tauri::command]
pub async fn dictionary_import_preview(
    state: State<'_, AppState>,
    path: String,
    format: RulesFormat,
) -> Result<ImportPreview, IpcError> {
    state
        .dictate
        .import_preview(&path, format == RulesFormat::Csv)
        .await
}

#[tauri::command]
pub async fn dictionary_import_commit(
    state: State<'_, AppState>,
    preview_id: String,
) -> Result<u32, IpcError> {
    Ok(state.dictate.import_commit(&preview_id).await? as u32)
}

#[tauri::command]
pub async fn dictionary_export(
    state: State<'_, AppState>,
    path: String,
    format: RulesFormat,
) -> Result<(), IpcError> {
    state
        .dictate
        .export_rules(&path, format == RulesFormat::Csv)
        .await
}

#[tauri::command]
pub async fn dictionary_suggestions(
    state: State<'_, AppState>,
) -> Result<Vec<Suggestion>, IpcError> {
    state.dictate.suggestions().await
}

/// F-DI-11: macOS's "Press 🌐 key to" preference.
#[tauri::command]
pub async fn dictate_globe_key(
    state: State<'_, AppState>,
) -> Result<crate::services::dictate::GlobeKeyView, IpcError> {
    Ok(state.dictate.globe_key().await)
}

/// `do_nothing: true` only after the consent dialog; `false` undoes it.
#[tauri::command]
pub async fn dictate_globe_key_set(
    state: State<'_, AppState>,
    do_nothing: bool,
) -> Result<crate::services::dictate::GlobeKeyView, IpcError> {
    state.dictate.globe_key_set(do_nothing).await
}

/// F-DI-09: one sync now (hosted: opted in, signed in).
#[tauri::command]
pub async fn dictionary_sync(
    state: State<'_, AppState>,
) -> Result<crate::services::dictionary_sync::SyncResult, IpcError> {
    state.dictate.dictionary_sync().await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Mine,
    Theirs,
}

#[tauri::command]
pub async fn dictionary_sync_resolve(
    state: State<'_, AppState>,
    id: String,
    keep: Keep,
) -> Result<Option<Rule>, IpcError> {
    state
        .dictate
        .dictionary_sync_resolve(&id, keep == Keep::Mine)
        .await
}

#[tauri::command]
pub async fn dictionary_sync_forget(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.dictate.dictionary_sync_forget().await
}

#[tauri::command]
pub async fn dictionary_suggestion_resolve(
    state: State<'_, AppState>,
    id: String,
    remember: bool,
) -> Result<Option<Rule>, IpcError> {
    state.dictate.resolve_suggestion(&id, remember).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Permission {
    Granted,
    Denied,
    Unknown,
    NotNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionStatus {
    pub mic: Permission,
    pub input_monitoring: Permission,
    pub accessibility: Permission,
}

#[tauri::command]
pub async fn dictate_permissions() -> Result<PermissionStatus, IpcError> {
    // Checked off the main thread (F-DI-03).
    tokio::task::spawn_blocking(permissions)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
}

fn permissions() -> PermissionStatus {
    permission_status(tandem_voice::permissions::system().as_deref())
}

fn to_permission(access: Access) -> Permission {
    match access {
        Access::Granted => Permission::Granted,
        Access::Denied => Permission::Denied,
        Access::NotAsked => Permission::Unknown,
    }
}

/// What the OS says about each grant; all "not needed" where the OS has no
/// such permissions (`probe` is `None` off macOS).
pub fn permission_status(probe: Option<&dyn PrivacyProbe>) -> PermissionStatus {
    match probe {
        None => PermissionStatus {
            mic: Permission::NotNeeded,
            input_monitoring: Permission::NotNeeded,
            accessibility: Permission::NotNeeded,
        },
        Some(p) => PermissionStatus {
            mic: to_permission(p.access(Privacy::Microphone)),
            input_monitoring: to_permission(p.access(Privacy::InputMonitoring)),
            accessibility: to_permission(p.access(Privacy::Accessibility)),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionKind {
    Microphone,
    InputMonitoring,
    Accessibility,
}

fn privacy(kind: PermissionKind) -> Privacy {
    match kind {
        PermissionKind::Microphone => Privacy::Microphone,
        PermissionKind::InputMonitoring => Privacy::InputMonitoring,
        PermissionKind::Accessibility => Privacy::Accessibility,
    }
}

/// System Settings › Privacy & Security, at the permission's list.
pub fn mac_permission_url(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        PermissionKind::InputMonitoring => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
        PermissionKind::Accessibility => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
    }
}

/// The OS settings page for a permission.
pub fn permission_url(kind: PermissionKind) -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(mac_permission_url(kind))
    } else if cfg!(windows) && kind == PermissionKind::Microphone {
        Some("ms-settings:privacy-microphone")
    } else {
        None
    }
}

#[tauri::command]
pub fn dictate_open_permission_settings(
    state: State<'_, AppState>,
    kind: PermissionKind,
) -> Result<(), IpcError> {
    let probe = tandem_voice::permissions::system();
    grant_permission(probe.as_deref(), kind, &|url| state.opener.open(url)).map(|_| ())
}

/// "Grant" (F-DI-03, D-2108). On a Mac: ask first, which shows the system
/// prompt and lists Vibezeo in System Settings, and open System Settings
/// only once the user has said no. Elsewhere: the OS page, if there is one.
pub fn grant_permission(
    probe: Option<&dyn PrivacyProbe>,
    kind: PermissionKind,
    open: &dyn Fn(&str) -> Result<(), String>,
) -> Result<GrantStep, IpcError> {
    let open_url = |url: &str| open(url).map_err(|e| IpcError::new(ErrorCode::Io, e));
    let Some(probe) = probe else {
        let url = permission_url(kind).ok_or_else(|| {
            IpcError::new(ErrorCode::Unavailable, "Nothing to grant on this system")
        })?;
        open_url(url)?;
        return Ok(GrantStep::OpenSettings);
    };
    let what = privacy(kind);
    let step = grant_step(what, probe.access(what));
    match step {
        GrantStep::Nothing => {}
        GrantStep::Prompt => probe.request(what),
        GrantStep::OpenSettings => open_url(mac_permission_url(kind))?,
        GrantStep::PromptAndOpenSettings => {
            probe.request(what);
            open_url(mac_permission_url(kind))?;
        }
    }
    Ok(step)
}

/// macOS: Input Monitoring takes effect on relaunch ("Quit & Reopen").
#[tauri::command]
pub fn dictate_relaunch(app: AppHandle) -> Result<(), IpcError> {
    // A sync command runs on the main thread, where restart() skips the
    // exit event: end agent runs first (F-TL-12).
    crate::services::agent_run::shutdown_all();
    app.restart();
}

#[tauri::command]
pub fn dictate_widget_set_position(
    state: State<'_, AppState>,
    x: i32,
    y: i32,
    locked: bool,
) -> Result<(), IpcError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn(async move {
        let _ = tandem_store::repo::settings::set(
            &store,
            tandem_core::settings::SettingKey::DictateWidgetPosition,
            serde_json::json!(format!("{x},{y}")),
        )
        .await;
        let _ = tandem_store::repo::settings::set(
            &store,
            tandem_core::settings::SettingKey::DictateLockPosition,
            serde_json::json!(locked),
        )
        .await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_03_permission_pages_per_os() {
        let mic = permission_url(PermissionKind::Microphone);
        if cfg!(target_os = "linux") {
            assert_eq!(mic, None);
        }
        if cfg!(windows) {
            assert_eq!(mic, Some("ms-settings:privacy-microphone"));
        }
    }

    #[test]
    fn f_di_03_grant_asks_before_it_opens_system_settings() {
        use tandem_voice::permissions::FakePrivacy;
        let opened = std::sync::Mutex::new(Vec::<String>::new());
        let open = |url: &str| -> Result<(), String> {
            opened
                .lock()
                .map_err(|e| e.to_string())?
                .push(url.to_string());
            Ok(())
        };
        let probe = FakePrivacy::with(&[
            (Privacy::Microphone, Access::NotAsked),
            (Privacy::InputMonitoring, Access::Denied),
            (Privacy::Accessibility, Access::Granted),
        ]);
        // Never asked: the system prompt, and System Settings stays shut.
        let step = grant_permission(Some(&probe), PermissionKind::Microphone, &open).expect("mic");
        assert_eq!(step, GrantStep::Prompt);
        assert_eq!(*probe.requests.lock(), vec![Privacy::Microphone]);
        assert!(opened.lock().expect("lock").is_empty());
        // Refused before: only System Settings can change it.
        let step = grant_permission(Some(&probe), PermissionKind::InputMonitoring, &open)
            .expect("input monitoring");
        assert_eq!(step, GrantStep::OpenSettings);
        assert_eq!(
            opened.lock().expect("lock").as_slice(),
            ["x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"]
        );
        // Accessibility can't tell the two apart: both.
        probe.set(Privacy::Accessibility, Access::NotAsked);
        let step =
            grant_permission(Some(&probe), PermissionKind::Accessibility, &open).expect("ax");
        assert_eq!(step, GrantStep::PromptAndOpenSettings);
        assert_eq!(probe.requests.lock().last(), Some(&Privacy::Accessibility));
        assert!(opened
            .lock()
            .expect("lock")
            .last()
            .is_some_and(|u| u.ends_with("Privacy_Accessibility")));
        // The status the UI shows.
        let status = permission_status(Some(&probe));
        assert_eq!(status.mic, Permission::Unknown);
        assert_eq!(status.input_monitoring, Permission::Denied);
        assert_eq!(status.accessibility, Permission::Unknown);
        assert_eq!(permission_status(None).mic, Permission::NotNeeded);
    }

    #[test]
    fn f_di_03_without_these_permissions_grant_opens_the_os_page_or_refuses() {
        let open = |_: &str| -> Result<(), String> { Ok(()) };
        let r = grant_permission(None, PermissionKind::Accessibility, &open);
        if cfg!(target_os = "macos") {
            assert!(r.is_ok());
        } else {
            assert!(r.is_err(), "nothing to grant here");
        }
    }

    #[test]
    fn share_cards_must_be_png() {
        let dir = tempfile::tempdir().expect("dir");
        let p = dir.path().join("card.png").to_string_lossy().into_owned();
        assert!(dictate_share_card_save(p.clone(), "aGVsbG8=".into()).is_err());
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nrest");
        dictate_share_card_save(p.clone(), format!("data:image/png;base64,{png}")).expect("saved");
        assert!(std::fs::read(&p).expect("read").starts_with(b"\x89PNG"));
    }
}
