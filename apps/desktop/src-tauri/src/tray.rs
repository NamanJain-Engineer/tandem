//! The tray / menu-bar icon (F-SH-13): shown while Dictate or background
//! routines are on (PRODUCT_SPEC), with Open Vibezeo, the Dictate status
//! line (it opens Settings › Dictate), Pause routines (while background
//! routines are on) and Quit. One tray for both: M8's Dictate and M11's
//! routines each built their own under the same id, so either could remove
//! the other's (D-819). "Pause routines" is the `pause_all_routines`
//! setting, so Settings, the palette and the tray agree.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tandem_core::bus::{BusEvent, Topic};
use tandem_core::settings::SettingKey;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::state::AppState;

const TRAY_ID: &str = "main";
const OPEN: &str = "tray-open";
const DICTATE: &str = "tray-dictate";
const PAUSE: &str = "tray-pause-routines";
const QUIT: &str = "tray-quit";

/// What a tray menu item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Dictate,
    TogglePause,
    Quit,
}

pub fn action(id: &str) -> Option<TrayAction> {
    match id {
        OPEN => Some(TrayAction::Open),
        DICTATE => Some(TrayAction::Dictate),
        PAUSE => Some(TrayAction::TogglePause),
        QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

/// The tray shows while Dictate or background routines are on.
pub fn wanted(background_routines: bool, dictate_on: bool) -> bool {
    background_routines || dictate_on
}

/// The menu's items, in order (`None` is a separator): "Pause routines"
/// only while background routines are on, since nothing else runs them.
pub fn layout(background_routines: bool) -> Vec<Option<&'static str>> {
    let mut items = vec![Some(OPEN), Some(DICTATE)];
    if background_routines {
        items.push(Some(PAUSE));
    }
    items.extend([None, Some(QUIT)]);
    items
}

/// What the tray shows; Dictate reports through `dictate_changed`, the
/// routines commands through `sync`.
struct Shown {
    routines: AtomicBool,
    dictate_on: AtomicBool,
    dictate_line: Mutex<String>,
    /// The live items, kept so a change can retext or tick them.
    status: Mutex<Option<MenuItem<Wry>>>,
    pause: Mutex<Option<CheckMenuItem<Wry>>>,
}

fn shown() -> &'static Shown {
    static SHOWN: OnceLock<Shown> = OnceLock::new();
    SHOWN.get_or_init(|| Shown {
        routines: AtomicBool::new(false),
        // Dictate is on by default; its first status says otherwise.
        dictate_on: AtomicBool::new(true),
        dictate_line: Mutex::new("Dictate: starting…".into()),
        status: Mutex::new(None),
        pause: Mutex::new(None),
    })
}

static APP: OnceLock<AppHandle> = OnceLock::new();

/// One show at a time: startup refreshes from Dictate and routines run on
/// different threads, and two that both found no tray would each build one.
static SHOWING: Mutex<()> = Mutex::new(());

/// At startup: remembers the app and shows the tray if it's wanted.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move { refresh(&app).await });
}

/// Dictate's status changed (`DictateService::emit_status`).
pub fn dictate_changed(on: bool, line: String) {
    let s = shown();
    if let Ok(mut l) = s.dictate_line.lock() {
        l.clone_from(&line);
    }
    let was = s.dictate_on.swap(on, Ordering::SeqCst);
    if let Some(item) = s.status.lock().ok().and_then(|i| i.clone()) {
        let _ = item.set_text(&line);
    }
    if was != on {
        if let Some(app) = APP.get().cloned() {
            tauri::async_runtime::spawn(async move { refresh(&app).await });
        }
    }
}

async fn paused(state: &AppState) -> bool {
    tandem_store::repo::settings::get(&state.store, SettingKey::PauseAllRoutines)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Background routines turned on or off, or "Pause routines" changed:
/// shows, rebuilds or removes the tray.
pub async fn sync(app: &AppHandle, background_routines: bool) {
    shown()
        .routines
        .store(background_routines, Ordering::SeqCst);
    refresh(app).await;
}

async fn refresh(app: &AppHandle) {
    let s = shown();
    let routines = s.routines.load(Ordering::SeqCst);
    if !wanted(routines, s.dictate_on.load(Ordering::SeqCst)) {
        let _one = SHOWING.lock().unwrap_or_else(|e| e.into_inner());
        let _ = app.remove_tray_by_id(TRAY_ID);
        return;
    }
    let state = app.state::<AppState>();
    let paused = paused(&state).await;
    if let Err(e) = show(app, routines, paused) {
        tracing::warn!(code = "tray.failed", error = %e, "couldn't show the tray icon");
    }
}

fn show(app: &AppHandle, routines: bool, paused: bool) -> tauri::Result<()> {
    let _one = SHOWING.lock().unwrap_or_else(|e| e.into_inner());
    let s = shown();
    let line = s.dictate_line.lock().map(|l| l.clone()).unwrap_or_default();
    let open = MenuItem::with_id(app, OPEN, "Open Vibezeo", true, None::<&str>)?;
    let status = MenuItem::with_id(app, DICTATE, line, true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, PAUSE, "Pause routines", true, paused, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit Vibezeo", true, None::<&str>)?;
    let items: Vec<&dyn IsMenuItem<Wry>> = layout(routines)
        .into_iter()
        .map(|id| -> &dyn IsMenuItem<Wry> {
            match id {
                Some(OPEN) => &open,
                Some(DICTATE) => &status,
                Some(PAUSE) => &pause,
                Some(_) => &quit,
                None => &sep,
            }
        })
        .collect();
    let menu = Menu::with_items(app, &items)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    } else {
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
    }
    if let Ok(mut i) = s.status.lock() {
        *i = Some(status);
    }
    if let Ok(mut i) = s.pause.lock() {
        *i = routines.then_some(pause);
    }
    Ok(())
}

async fn run(app: &AppHandle, act: TrayAction) {
    match act {
        TrayAction::Open => crate::focus_main(app),
        TrayAction::Dictate => {
            crate::focus_main(app);
            let _ = app.emit_to(
                "main",
                "ui:open-settings",
                serde_json::json!({ "page": "dictate" }),
            );
        }
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
            if let Some(item) = shown().pause.lock().ok().and_then(|i| i.clone()) {
                let _ = item.set_checked(value.as_bool().unwrap_or(false));
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
        assert_eq!(action("tray-dictate"), Some(TrayAction::Dictate));
        assert_eq!(action("tray-pause-routines"), Some(TrayAction::TogglePause));
        assert_eq!(action("tray-quit"), Some(TrayAction::Quit));
        assert_eq!(action("other"), None);
    }

    #[test]
    fn f_sh_13_the_tray_shows_while_dictate_or_background_routines_are_on() {
        assert!(wanted(true, false));
        assert!(wanted(false, true));
        assert!(wanted(true, true));
        assert!(!wanted(false, false));
    }

    #[test]
    fn f_sh_13_pause_routines_is_in_the_menu_only_with_background_routines() {
        assert_eq!(
            layout(false),
            vec![
                Some("tray-open"),
                Some("tray-dictate"),
                None,
                Some("tray-quit")
            ]
        );
        assert_eq!(
            layout(true),
            vec![
                Some("tray-open"),
                Some("tray-dictate"),
                Some("tray-pause-routines"),
                None,
                Some("tray-quit")
            ]
        );
    }
}
