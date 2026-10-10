//! The tray / menu-bar icon (F-SH-13): shown while background routines are
//! on, with Open Vibezeo, Pause routines and Quit. "Pause routines" is the
//! `pause_all_routines` setting, so Settings, the palette and the tray agree.

use tandem_core::bus::{BusEvent, Topic};
use tandem_core::settings::SettingKey;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::state::AppState;

const TRAY_ID: &str = "main";
const OPEN: &str = "tray-open";
const PAUSE: &str = "tray-pause-routines";
const QUIT: &str = "tray-quit";

/// What a tray menu item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    TogglePause,
    Quit,
}

pub fn action(id: &str) -> Option<TrayAction> {
    match id {
        OPEN => Some(TrayAction::Open),
        PAUSE => Some(TrayAction::TogglePause),
        QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

/// The tray shows while background routines are on (Dictate joins later).
pub fn wanted(background_routines: bool) -> bool {
    background_routines
}

/// The pause item, kept so a settings change can tick it.
struct PauseItem(CheckMenuItem<Wry>);

async fn paused(state: &AppState) -> bool {
    tandem_store::repo::settings::get(&state.store, SettingKey::PauseAllRoutines)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Shows or removes the tray, and ticks "Pause routines" to match the
/// setting.
pub async fn sync(app: &AppHandle, background_routines: bool) {
    let state = app.state::<AppState>();
    let paused = paused(&state).await;
    if !wanted(background_routines) {
        let _ = app.remove_tray_by_id(TRAY_ID);
        return;
    }
    if app.tray_by_id(TRAY_ID).is_some() {
        if let Some(item) = app.try_state::<PauseItem>() {
            let _ = item.0.set_checked(paused);
        }
        return;
    }
    if let Err(e) = build(app, paused) {
        tracing::warn!(code = "tray.failed", error = %e, "couldn't show the tray icon");
    }
}

fn build(app: &AppHandle, paused: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, "Open Vibezeo", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, PAUSE, "Pause routines", true, paused, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &pause, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Vibezeo")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let Some(act) = action(event.id().as_ref()) else {
                return;
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move { run(&app, act).await });
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    if app.try_state::<PauseItem>().is_none() {
        app.manage(PauseItem(pause));
    }
    Ok(())
}

async fn run(app: &AppHandle, act: TrayAction) {
    match act {
        TrayAction::Open => crate::focus_main(app),
        TrayAction::Quit => app.exit(0),
        TrayAction::TogglePause => {
            let state = app.state::<AppState>();
            let value = serde_json::Value::Bool(!paused(&state).await);
            if let Err(e) = tandem_store::repo::settings::set(
                &state.store,
                SettingKey::PauseAllRoutines,
                value.clone(),
            )
            .await
            {
                tracing::warn!(code = "tray.pause_failed", error = %e, "couldn't pause routines");
                return;
            }
            state.bus.publish(BusEvent::new(
                Topic::App,
                "settings:changed",
                serde_json::json!({ "key": SettingKey::PauseAllRoutines, "value": value }),
            ));
            state.routines.notify_changed();
            if let Some(item) = app.try_state::<PauseItem>() {
                let _ = item.0.set_checked(value.as_bool().unwrap_or(false));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_sh_13_tray_items_map_to_actions() {
        assert_eq!(action("tray-open"), Some(TrayAction::Open));
        assert_eq!(action("tray-pause-routines"), Some(TrayAction::TogglePause));
        assert_eq!(action("tray-quit"), Some(TrayAction::Quit));
        assert_eq!(action("other"), None);
    }

    #[test]
    fn f_sh_13_the_tray_shows_only_with_background_routines() {
        assert!(wanted(true));
        assert!(!wanted(false));
    }
}
