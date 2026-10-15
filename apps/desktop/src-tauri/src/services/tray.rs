//! The tray's Dictate status line (F-SH-13, MILESTONES M8 task 11). The
//! tray itself is `crate::tray`, shared with background routines.

/// The status line for a `DictateStatus`.
pub fn status_line(status: &crate::services::dictate::DictateStatus) -> String {
    use tandem_voice::hotkey::HotkeyStatus;
    if !status.enabled {
        return "Dictate: off".into();
    }
    if status.model.is_none() && status.engine != "cloud" {
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

/// Keeps the tray's Dictate line current, and the tray shown while
/// Dictate is on (`crate::tray`, D-819).
pub fn update(status: &crate::services::dictate::DictateStatus) {
    crate::tray::dictate_changed(status.enabled, status_line(status));
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
        // Cloud dictation needs no local model.
        s.engine = "cloud".into();
        assert_eq!(status_line(&s), "Dictate: ready · hold Ctrl+Alt");
        let mut s = status();
        s.enabled = false;
        assert_eq!(status_line(&s), "Dictate: off");
        let mut s = status();
        s.hotkey = HotkeyStatus::Unavailable { reason: "x".into() };
        assert_eq!(status_line(&s), "Dictate: shortcut unavailable");
    }
}
