//! The window's web content stopping (F-SH-07.8, D-2564). WebKitGTK's web
//! process crashing, or going over its memory limit, leaves a blank window
//! while the core and every agent go on. The window reloads by itself, and
//! each pane attaches again to its running process with what it wrote
//! meanwhile (the reload `pty_attach` and `status_list` are made for). After
//! [`MAX_RELOADS`] reloads within [`WITHIN`], the next stop asks first.
//! Linux follows WebKitGTK's `web-process-terminated`, macOS Tauri's
//! content-process hook; Windows' WebView2 isn't followed yet.

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

/// Reloads made by themselves within [`WITHIN`]; the stop after them asks.
pub const MAX_RELOADS: usize = 2;
/// How far back [`MAX_RELOADS`] counts.
pub const WITHIN: Duration = Duration::from_secs(5 * 60);

const TITLE: &str = "Vibezeo's window keeps stopping";
const BODY: &str = "Its content stopped 3 times in 5 minutes. Your agents are still running. Reload tries again; Quit Vibezeo stops them, and they come back paused when you open it again.";

/// Why the window's web content stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// It crashed, or the system ended it.
    Crashed,
    /// It went over WebKit's memory limit.
    OutOfMemory,
    /// Vibezeo ended it itself: nothing to recover.
    Ours,
}

/// What a stop does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    Reload,
    /// Too many in a row: ask before reloading again.
    Ask,
    Nothing,
}

/// The reloads made by themselves lately.
#[derive(Debug, Default)]
pub struct Reloads {
    at: Mutex<VecDeque<Instant>>,
}

impl Reloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// What a stop at `now` does: a reload, unless [`MAX_RELOADS`] came
    /// within [`WITHIN`] before it. Asking doesn't count as one.
    pub fn on_end(&self, ended: Ended, now: Instant) -> Recovery {
        if ended == Ended::Ours {
            return Recovery::Nothing;
        }
        let mut at = self.at.lock().unwrap_or_else(|e| e.into_inner());
        while at
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= WITHIN)
        {
            at.pop_front();
        }
        if at.len() >= MAX_RELOADS {
            return Recovery::Ask;
        }
        at.push_back(now);
        Recovery::Reload
    }

    /// Reload, chosen when asked: the count starts again.
    pub fn restart(&self) {
        self.at.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

static RELOADS: LazyLock<Reloads> = LazyLock::new(Reloads::new);

/// The main window's web content stopped: reloads it, or asks first after
/// too many in a row (Reload, or Quit Vibezeo).
pub fn ended(app: &AppHandle, ended: Ended) {
    let recovery = RELOADS.on_end(ended, Instant::now());
    tracing::warn!(
        code = "window.web_ended",
        ended = ?ended,
        recovery = ?recovery,
        "the window's web content stopped"
    );
    match recovery {
        Recovery::Nothing => {}
        Recovery::Reload => reload(app),
        Recovery::Ask => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let again = crate::commands::lanes::confirm_native_as(
                    &app,
                    TITLE,
                    BODY,
                    "Reload",
                    "Quit Vibezeo",
                )
                .await;
                if again {
                    RELOADS.restart();
                    reload(&app);
                } else {
                    crate::services::background::quit(&app);
                }
            });
        }
    }
}

fn reload(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(err) = window.reload() {
        tracing::warn!(code = "window.reload_failed", error = %err, "couldn't reload the window");
    }
}

/// Linux: follows the main webview's `web-process-terminated`.
#[cfg(target_os = "linux")]
pub fn watch(window: &tauri::WebviewWindow) {
    let app = window.app_handle().clone();
    let done = window.with_webview(move |platform| {
        use webkit2gtk::{WebProcessTerminationReason as Reason, WebViewExt};
        platform
            .inner()
            .connect_web_process_terminated(move |_, reason| {
                let why = match reason {
                    Reason::TerminatedByApi => Ended::Ours,
                    Reason::ExceededMemoryLimit => Ended::OutOfMemory,
                    _ => Ended::Crashed,
                };
                ended(&app, why);
            });
    });
    if let Err(err) = done {
        tracing::warn!(code = "window.no_watch", error = %err, "couldn't follow the window's web content");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn f_sh_07_8_a_crash_reloads_the_window() {
        let r = Reloads::new();
        let t = Instant::now();
        assert_eq!(r.on_end(Ended::Crashed, t), Recovery::Reload);
        assert_eq!(r.on_end(Ended::OutOfMemory, at(t, 1)), Recovery::Reload);
    }

    #[test]
    fn f_sh_07_8_a_third_stop_within_five_minutes_asks() {
        let r = Reloads::new();
        let t = Instant::now();
        assert_eq!(r.on_end(Ended::Crashed, t), Recovery::Reload);
        assert_eq!(r.on_end(Ended::Crashed, at(t, 60)), Recovery::Reload);
        assert_eq!(r.on_end(Ended::Crashed, at(t, 120)), Recovery::Ask);
        // Still asking, and asking adds nothing to the count.
        assert_eq!(r.on_end(Ended::Crashed, at(t, 121)), Recovery::Ask);
        // Five minutes after the first, it reloads by itself again.
        assert_eq!(r.on_end(Ended::Crashed, at(t, 300)), Recovery::Reload);
    }

    #[test]
    fn f_sh_07_8_reload_chosen_when_asked_starts_the_count_again() {
        let r = Reloads::new();
        let t = Instant::now();
        r.on_end(Ended::Crashed, t);
        r.on_end(Ended::Crashed, at(t, 1));
        assert_eq!(r.on_end(Ended::Crashed, at(t, 2)), Recovery::Ask);
        r.restart();
        assert_eq!(r.on_end(Ended::Crashed, at(t, 3)), Recovery::Reload);
    }

    #[test]
    fn f_sh_07_8_content_vibezeo_ended_itself_is_left_alone() {
        let r = Reloads::new();
        let t = Instant::now();
        assert_eq!(r.on_end(Ended::Ours, t), Recovery::Nothing);
        // It doesn't count either.
        assert_eq!(r.on_end(Ended::Crashed, at(t, 1)), Recovery::Reload);
        assert_eq!(r.on_end(Ended::Crashed, at(t, 2)), Recovery::Reload);
    }
}
