//! Agents that keep running when the window closes (F-SH-14, D-2560): the
//! window hides and Vibezeo itself keeps running, so the agents, their
//! notifications, approvals, the phone and routines go on as with it open.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tandem_core::settings::SettingKey;
use tandem_core::status::{AgentStatus, StatusRecord};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use crate::state::AppState;

/// How often the tray's count is checked while the window is hidden.
const RECOUNT: Duration = Duration::from_secs(5);

/// How long the window has to show the close dialog's question before the
/// close goes ahead without it (D-2563).
const SEEN_WAIT: Duration = Duration::from_secs(5);

/// An agent the close would stop: a pane or thread working or waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunningAgent {
    /// `pane:<id>` or `thread:<id>`.
    pub key: String,
    pub title: Option<String>,
}

/// `app:close-requested`: the close dialog's question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloseRequest {
    /// The window says it showed it with `app_close_seen`.
    pub id: u32,
    pub agents: Vec<RunningAgent>,
    /// The menu's Quit rather than the window's close button.
    pub quit: bool,
}

/// What a close does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseAction {
    /// Close or quit as before: nothing would be lost, or the setting says stop.
    Proceed,
    /// Ask with the close dialog.
    Ask(Vec<RunningAgent>),
    /// Hide the window and keep these running.
    Keep(Vec<RunningAgent>),
}

/// The agents a close would stop (D-2561): panes whose process is `live` and
/// threads, working or waiting. Done, idle and failed ones lose nothing.
pub fn running_agents(records: &[StatusRecord], live: impl Fn(&str) -> bool) -> Vec<RunningAgent> {
    records
        .iter()
        .filter(|r| matches!(r.status, AgentStatus::Working | AgentStatus::Waiting))
        .filter(|r| match r.key.0.strip_prefix("pane:") {
            Some(pane) => live(pane),
            None => r.key.0.starts_with("thread:"),
        })
        .map(|r| RunningAgent {
            key: r.key.0.clone(),
            title: r.title.clone(),
        })
        .collect()
}

/// What closing the window (or the menu's Quit) does: with `in_tray` (the
/// "Keep running in the tray when the window closes" switch, never for Quit)
/// the window hides whatever runs; otherwise `keep_agents_on_close` (`ask`,
/// `keep` or `stop`) decides for `agents`, and with none it closes as before.
pub fn close_action(in_tray: bool, setting: &str, agents: Vec<RunningAgent>) -> CloseAction {
    if in_tray {
        return CloseAction::Keep(agents);
    }
    if agents.is_empty() {
        return CloseAction::Proceed;
    }
    match setting {
        "keep" => CloseAction::Keep(agents),
        "stop" => CloseAction::Proceed,
        _ => CloseAction::Ask(agents),
    }
}

/// What a question the window never showed does (D-2563): keeps the agents
/// still running with the window hidden, or quits once none runs.
pub fn unshown(agents: Vec<RunningAgent>) -> CloseAction {
    if agents.is_empty() {
        CloseAction::Proceed
    } else {
        CloseAction::Keep(agents)
    }
}

/// The close dialog's questions: the last one asked and the last one the
/// window said it showed.
pub struct Questions {
    asked: AtomicU32,
    seen: AtomicU32,
}

impl Questions {
    pub const fn new() -> Self {
        Self {
            asked: AtomicU32::new(0),
            seen: AtomicU32::new(0),
        }
    }

    /// A new question's number.
    pub fn ask(&self) -> u32 {
        self.asked.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
    }

    /// The window showed question `id`.
    pub fn seen(&self, id: u32) {
        self.seen.fetch_max(id, Ordering::SeqCst);
    }

    /// Whether the window showed question `id`, or a later one.
    pub fn shown(&self, id: u32) -> bool {
        self.seen.load(Ordering::SeqCst) >= id
    }
}

impl Default for Questions {
    fn default() -> Self {
        Self::new()
    }
}

static QUESTIONS: Questions = Questions::new();

/// Asks with the close dialog (`app:close-requested`). A question the window
/// doesn't show within `SEEN_WAIT`, from a hung or reloading webview, goes
/// ahead without it (`unshown`), so a close is never stuck (D-2563).
pub fn ask(app: &AppHandle, agents: Vec<RunningAgent>, quit: bool) {
    let id = QUESTIONS.ask();
    let _ = app.emit_to(
        "main",
        "app:close-requested",
        CloseRequest { id, agents, quit },
    );
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SEEN_WAIT).await;
        if QUESTIONS.shown(id) {
            return;
        }
        tracing::warn!(
            code = "close.unshown",
            "the window didn't show the close question"
        );
        match unshown(running_now(&app.state::<AppState>())) {
            CloseAction::Keep(agents) => keep(&app, agents.len()),
            _ => self::quit(&app),
        }
    });
}

/// `app_close_seen`: the window showed question `id`.
pub fn seen(id: u32) {
    QUESTIONS.seen(id);
}

/// Stops every agent and quits, as `app_quit`: scrollback snapshots, then
/// every process ends.
pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.terminals.shutdown(&state.data.scrollback());
    app.exit(0);
}

/// The setting "Don't ask again" saves for a choice.
pub fn remembered(keep: bool) -> &'static str {
    if keep {
        "keep"
    } else {
        "stop"
    }
}

/// The agents running now: the status store's sessions, panes checked
/// against their PTYs.
pub fn running_now(state: &AppState) -> Vec<RunningAgent> {
    running_agents(&state.terminals.status.snapshot(), |pane| {
        state.terminals.pty.is_live(pane)
    })
}

/// What closing does now; `quit` is the menu's Quit, which the tray switch
/// never keeps. Blocking settings reads.
pub fn decide(state: &AppState, quit: bool) -> CloseAction {
    let get = |key| tandem_store::repo::settings::get_blocking(&state.store, key).ok();
    let in_tray = !quit
        && get(SettingKey::KeepRunningInTray)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let setting = get(SettingKey::KeepAgentsOnClose)
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    close_action(in_tray, &setting, running_now(state))
}

/// The window is hidden with Vibezeo running.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// Keep running (F-SH-14): hides the window and shows the tray with how
/// many agents run, recounted while it's hidden so the count follows the
/// ones that finish. Notifications treat the window as unfocused.
pub fn keep(app: &AppHandle, agents: usize) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    app.state::<AppState>().notifications.set_focused(false);
    let first = !HIDDEN.swap(true, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::tray::background(&app, true, agents).await;
        if !first {
            return;
        }
        loop {
            tokio::time::sleep(RECOUNT).await;
            if !HIDDEN.load(Ordering::SeqCst) {
                break;
            }
            let n = running_now(&app.state::<AppState>()).len();
            crate::tray::background(&app, true, n).await;
        }
    });
}

/// The window is shown again: the tray drops the count.
pub fn shown(app: &AppHandle) {
    if HIDDEN.swap(false, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { crate::tray::background(&app, false, 0).await });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::status::{SessionKey, WaitingReason};

    fn record(key: &str, status: AgentStatus) -> StatusRecord {
        StatusRecord {
            key: SessionKey(key.into()),
            status,
            reason: (status == AgentStatus::Waiting).then_some(WaitingReason::Approval),
            since: 0,
            ts: 0,
            tool: None,
            title: Some(format!("{key} title")),
            preview: None,
        }
    }

    fn agent(key: &str) -> RunningAgent {
        RunningAgent {
            key: key.into(),
            title: Some(format!("{key} title")),
        }
    }

    #[test]
    fn f_sh_14_1_working_and_waiting_agents_with_a_live_process_are_running() {
        let records = [
            record("pane:a", AgentStatus::Working),
            record("pane:b", AgentStatus::Waiting),
            record("pane:c", AgentStatus::Done),
            record("pane:d", AgentStatus::Idle),
            record("pane:e", AgentStatus::Failed),
            // Its process ended: a close loses nothing.
            record("pane:f", AgentStatus::Working),
            record("thread:t", AgentStatus::Working),
            // Not an agent session.
            record("routine:r", AgentStatus::Working),
        ];
        let live = |pane: &str| pane != "f";
        assert_eq!(
            running_agents(&records, live),
            [agent("pane:a"), agent("pane:b"), agent("thread:t")]
        );
    }

    #[test]
    fn f_sh_14_1_a_close_with_agents_running_asks_unless_the_setting_says() {
        let two = vec![agent("pane:a"), agent("pane:b")];
        assert_eq!(
            close_action(false, "ask", two.clone()),
            CloseAction::Ask(two.clone())
        );
        assert_eq!(
            close_action(false, "keep", two.clone()),
            CloseAction::Keep(two.clone())
        );
        assert_eq!(
            close_action(false, "stop", two.clone()),
            CloseAction::Proceed
        );
        // An unknown value asks, as the default does.
        assert_eq!(
            close_action(false, "other", two.clone()),
            CloseAction::Ask(two)
        );
    }

    #[test]
    fn f_sh_14_4_with_no_agent_running_a_close_never_asks() {
        for setting in ["ask", "keep", "stop"] {
            assert_eq!(
                close_action(false, setting, Vec::new()),
                CloseAction::Proceed
            );
        }
    }

    #[test]
    fn f_sh_14_keep_running_in_the_tray_hides_the_window_with_or_without_agents() {
        // Settings › General's "Keep running in the tray when the window
        // closes" (UI_SPEC): the window hides whatever runs.
        assert_eq!(
            close_action(true, "stop", Vec::new()),
            CloseAction::Keep(Vec::new())
        );
        let two = vec![agent("pane:a"), agent("pane:b")];
        assert_eq!(
            close_action(true, "ask", two.clone()),
            CloseAction::Keep(two)
        );
    }

    #[test]
    fn f_sh_14_a_question_the_window_showed_is_answered_there() {
        let q = Questions::new();
        let first = q.ask();
        assert!(!q.shown(first));
        q.seen(first);
        assert!(q.shown(first));
    }

    #[test]
    fn f_sh_14_a_question_the_window_never_showed_stays_unshown() {
        let q = Questions::new();
        let first = q.ask();
        q.seen(first);
        // Showing the first doesn't show the next.
        let second = q.ask();
        assert!(!q.shown(second));
        // Showing a later one covers it; a late word for an older one
        // doesn't undo that.
        let third = q.ask();
        q.seen(third);
        q.seen(first);
        assert!(q.shown(second));
        assert!(q.shown(third));
    }

    #[test]
    fn f_sh_14_an_unshown_question_keeps_the_agents_still_running() {
        let two = vec![agent("pane:a"), agent("thread:t")];
        assert_eq!(unshown(two.clone()), CloseAction::Keep(two));
        // Nothing runs any more: nothing is lost by quitting.
        assert_eq!(unshown(Vec::new()), CloseAction::Proceed);
    }

    #[test]
    fn f_sh_14_5_dont_ask_again_saves_the_choice() {
        assert_eq!(remembered(true), "keep");
        assert_eq!(remembered(false), "stop");
    }
}
