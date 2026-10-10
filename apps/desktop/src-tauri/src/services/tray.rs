//! The tray icon (F-SH-13, MILESTONES M8 task 11): Open Vibezeo, the
//! Dictate status line (it opens Settings › Dictate) and Quit.

use std::sync::OnceLock;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Emitter;

static STATUS: OnceLock<MenuItem<tauri::Wry>> = OnceLock::new();

/// The status line for a `DictateStatus`.
pub fn status_line(status: &crate::services::dictate::DictateStatus) -> String {
    use tandem_voice::hotkey::HotkeyStatus;
    if !status.enabled {
        return "Dictate: off".into();
    }
    if status.model.is_none() {
        return "Dictate: download a model to start".into();
    }
    match &status.hotkey {
        HotkeyStatus::Active => format!("Dictate: ready · hold {}", status.hold.label),
        HotkeyStatus::Paused => "Dictate: paused (secure input)".into(),
        HotkeyStatus::NeedsPermission => "Dictate: needs Input Monitoring".into(),
        HotkeyStatus::Starting => "Dictate: starting…".into(),
        HotkeyStatus::Unavailable { .. } | HotkeyStatus::NotRunning => {
            "Dictate: shortcut unavailable".into()
        }
    }
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "tray-open", "Open Vibezeo", true, None::<&str>)?;
    let status = MenuItem::with_id(
        app,
        "tray-dictate",
        "Dictate: starting…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit Vibezeo", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &status, &sep, &quit])?;
    let _ = STATUS.set(status);
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Vibezeo")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-open" => crate::focus_main(app),
            "tray-dictate" => {
                crate::focus_main(app);
                let _ = app.emit_to(
                    "main",
                    "ui:open-settings",
                    serde_json::json!({ "page": "dictate" }),
                );
            }
            "tray-quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Keeps the status line current.
pub fn update(status: &crate::services::dictate::DictateStatus) {
    if let Some(item) = STATUS.get() {
        let _ = item.set_text(status_line(status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::dictate::{DictateStatus, KeyLabel, ModelRef, ModelStage};
    use tandem_voice::hotkey::HotkeyStatus;

    fn status() -> DictateStatus {
        DictateStatus {
            enabled: true,
            hotkey: HotkeyStatus::Active,
            verified: true,
            hold: KeyLabel {
                binding: "Ctrl+Alt".into(),
                label: "Ctrl+Alt".into(),
            },
            toggle: None,
            latch: true,
            session: "x11".into(),
            deep_link_command: None,
            mic: None,
            engine: "local".into(),
            model: Some(ModelRef {
                id: "whisper-base.en".into(),
                name: "Whisper Base (English)".into(),
            }),
            model_stage: ModelStage::Ready,
            fallback: None,
            state: tandem_voice::take::TakeState::Idle,
            missing_tools: None,
            can_pause_media: true,
            restores_clipboard: true,
            theme: "dark".into(),
            widget_style: "logo_text".into(),
            lock_position: false,
            quick_actions: false,
            enhance_disclosed: false,
        }
    }

    #[test]
    fn tray_status_lines_say_what_is_true() {
        assert_eq!(status_line(&status()), "Dictate: ready · hold Ctrl+Alt");
        let mut s = status();
        s.model = None;
        assert_eq!(status_line(&s), "Dictate: download a model to start");
        let mut s = status();
        s.enabled = false;
        assert_eq!(status_line(&s), "Dictate: off");
        let mut s = status();
        s.hotkey = HotkeyStatus::Unavailable { reason: "x".into() };
        assert_eq!(status_line(&s), "Dictate: shortcut unavailable");
    }
}
