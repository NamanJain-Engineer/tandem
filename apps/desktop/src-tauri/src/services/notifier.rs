//! OS notifications (F-DB-03): the `Notifier` trait, the system one (tauri
//! notification plugin) and a recording one for the test harness.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use tandem_core::source::SourceRef;
use tandem_store::repo::notifications::NotificationKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OsNotification {
    pub kind: NotificationKind,
    /// The item's title; the body is the source line only (UI-40: no
    /// message text).
    pub title: String,
    pub body: String,
    pub source: SourceRef,
}

pub trait Notifier: Send + Sync {
    fn notify(&self, n: &OsNotification);
    /// Dock bounce (macOS) or taskbar flash (Windows); background only.
    fn attention(&self);
}

/// The system notifier.
pub struct SystemNotifier {
    app: AppHandle,
}

impl SystemNotifier {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Notifier for SystemNotifier {
    fn notify(&self, n: &OsNotification) {
        use tauri_plugin_notification::NotificationExt;
        if let Err(err) = self
            .app
            .notification()
            .builder()
            .title(&n.title)
            .body(&n.body)
            .show()
        {
            tracing::warn!(error = %err, code = "notify.os_failed", "couldn't show a system notification");
        }
    }

    fn attention(&self) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
    }
}

/// Records calls instead of showing anything (E2E, F-DB-03 AC1).
#[derive(Default)]
pub struct RecordingNotifier {
    calls: Mutex<Vec<OsNotification>>,
    attention: Mutex<u32>,
}

impl RecordingNotifier {
    pub fn calls(&self) -> Vec<OsNotification> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn attention_count(&self) -> u32 {
        self.attention.lock().map(|a| *a).unwrap_or(0)
    }

    pub fn clear(&self) {
        if let Ok(mut c) = self.calls.lock() {
            c.clear();
        }
        if let Ok(mut a) = self.attention.lock() {
            *a = 0;
        }
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&self, n: &OsNotification) {
        if let Ok(mut c) = self.calls.lock() {
            c.push(n.clone());
        }
    }

    fn attention(&self) {
        if let Ok(mut a) = self.attention.lock() {
            *a += 1;
        }
    }
}
