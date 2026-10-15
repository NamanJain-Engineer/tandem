//! Context fill per agent pane (F-AC-06), read from the engine's own session
//! transcript when a turn ends, usage windows per account (F-AC-04), from
//! the engine where it reports them, and their forecasts and the sessions
//! spending them (F-AC-08).

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::settings::SettingKey;
use tandem_engines::forecast::{UsageHistory, WindowForecast};
use tandem_engines::transcript::ContextFill;
use tandem_engines::usage::UsageWindow;
use ts_rs::TS;

/// The latest context fill per pane.
pub struct ContextBoard {
    fills: Mutex<HashMap<String, ContextFill>>,
    bus: EventBus,
}

/// `context:changed`, and one row of `contexts_list`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PaneContext")]
pub struct PaneContextView {
    pub pane_id: String,
    pub used: u64,
    pub window: u64,
}

impl ContextBoard {
    pub fn new(bus: EventBus) -> Self {
        Self {
            fills: Mutex::default(),
            bus,
        }
    }

    pub fn set(&self, pane: &str, fill: ContextFill) {
        let changed = self
            .fills
            .lock()
            .map(|mut f| f.insert(pane.to_string(), fill) != Some(fill))
            .unwrap_or(false);
        if changed {
            self.bus.publish(BusEvent::new(
                Topic::Status,
                "context:changed",
                PaneContextView {
                    pane_id: pane.to_string(),
                    used: fill.used,
                    window: fill.window,
                },
            ));
        }
    }

    pub fn clear(&self, pane: &str) {
        if let Ok(mut f) = self.fills.lock() {
            f.remove(pane);
        }
    }

    pub fn list(&self) -> Vec<PaneContextView> {
        self.fills
            .lock()
            .map(|f| {
                f.iter()
                    .map(|(pane, fill)| PaneContextView {
                        pane_id: pane.clone(),
                        used: fill.used,
                        window: fill.window,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_ac_06_a_new_fill_is_sent_once_and_listed() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::Status);
        let board = ContextBoard::new(bus);
        let fill = ContextFill {
            used: 150_000,
            window: 200_000,
        };
        board.set("pn_1", fill);
        board.set("pn_1", fill);
        assert_eq!(rx.try_recv().expect("changed").name, "context:changed");
        assert!(rx.try_recv().is_err(), "the same fill isn't sent twice");
        assert_eq!(board.list().len(), 1);
        board.clear("pn_1");
        assert!(board.list().is_empty());
    }
}

/// Where an account's usage came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum UsageSource {
    CodexAppServer,
    Unavailable,
}

/// `usage_get` row and `usage:changed` payload (F-AC-04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageView {
    pub account_id: tandem_core::ids::AccountId,
    pub engine_id: String,
    pub windows: Vec<UsageWindow>,
    pub source: UsageSource,
    /// From local logs rather than the engine (none yet).
    pub estimated: bool,
    /// Why there's no data ("Usage unavailable" tooltip).
    pub reason: Option<String>,
    pub fetched_at: i64,
    /// One per window, in the same order (F-AC-08).
    pub forecasts: Vec<WindowForecast>,
}

/// The latest usage per account, the readings behind its forecasts, and
/// the windows already warned about (F-AC-08).
#[derive(Default)]
pub struct UsageBoard {
    views: Mutex<HashMap<String, UsageView>>,
    history: Mutex<UsageHistory>,
    /// `(account, window kind)` → the reset time of the window already
    /// notified. Engines report reset times that drift by seconds between
    /// readings, so a reset within a minute of it is the same window.
    warned: Mutex<HashMap<(String, String), i64>>,
}

impl UsageBoard {
    pub fn get(&self, account: &str) -> Option<UsageView> {
        self.views.lock().ok()?.get(account).cloned()
    }
    fn put(&self, view: &UsageView) {
        if let Ok(mut v) = self.views.lock() {
            v.insert(view.account_id.as_str().to_string(), view.clone());
        }
    }

    /// Records a reading and returns each window's forecast.
    pub fn record(&self, account: &str, windows: &[UsageWindow], now: i64) -> Vec<WindowForecast> {
        self.history
            .lock()
            .map(|mut h| h.record(account, windows, now))
            .unwrap_or_default()
    }

    /// The windows of `view` newly forecast to run out before their reset:
    /// each is returned once per account, window and reset time.
    pub fn newly_short(&self, view: &UsageView) -> Vec<(UsageWindow, WindowForecast)> {
        let Ok(mut warned) = self.warned.lock() else {
            return Vec::new();
        };
        let account = view.account_id.as_str();
        view.windows
            .iter()
            .zip(&view.forecasts)
            .filter(|(w, f)| {
                f.runs_out_early() && f.before_reset_ms.is_some() && w.resets_at.is_some()
            })
            .filter(|(w, _)| {
                let Some(reset) = w.resets_at else {
                    return false;
                };
                let key = (account.to_string(), w.kind.clone());
                match warned.get(&key) {
                    Some(old) if reset <= old + 60_000 => false,
                    _ => {
                        warned.insert(key, reset);
                        true
                    }
                }
            })
            .map(|(w, f)| (w.clone(), f.clone()))
            .collect()
    }

    /// The rise of the account's fullest window over the last hour (F-AC-09).
    pub fn hour_rise(&self, account: &str, now: i64) -> Option<f64> {
        self.history.lock().ok()?.hour_rise(account, now)
    }

    /// The test harness: earlier readings move `ms` into the past.
    pub fn backdate(&self, ms: i64) {
        if let Ok(mut h) = self.history.lock() {
            h.backdate(ms);
        }
    }

    /// Forgets a removed account.
    pub fn forget(&self, account: &str) {
        if let Ok(mut v) = self.views.lock() {
            v.remove(account);
        }
        if let Ok(mut h) = self.history.lock() {
            h.forget(account);
        }
        if let Ok(mut w) = self.warned.lock() {
            w.retain(|(a, _), _| a != account);
        }
    }
}

/// How long one probe may take.
const PROBE_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// Reads one account's usage from its engine and tells the webview.
pub async fn probe(
    state: &crate::state::AppState,
    account: &tandem_store::repo::accounts::Account,
) -> UsageView {
    let engines = &state.terminals.engines;
    let engine = engines.get(&account.engine_id);
    let label = engine
        .as_ref()
        .map(|e| e.label.clone())
        .unwrap_or_else(|| account.engine_id.clone());
    let mut view = UsageView {
        account_id: account.id.clone(),
        engine_id: account.engine_id.clone(),
        windows: Vec::new(),
        source: UsageSource::Unavailable,
        estimated: false,
        reason: Some(format!("{label} doesn't report usage to other apps.")),
        fetched_at: state.clock.now_ms(),
        forecasts: Vec::new(),
    };
    let codex = engine
        .as_ref()
        .filter(|e| e.usage_probe.as_deref() == Some("codex_app_server"));
    if let Some(engine) = codex {
        match state.terminals.resolve_engine(engine).await {
            None => view.reason = Some(format!("{label} isn't installed on this machine.")),
            Some(program) => {
                let home = std::path::PathBuf::from(&account.home_path);
                // A pane's launch prepares the home; a probe can come first.
                if let Err(e) = tokio::fs::create_dir_all(&home).await {
                    tracing::warn!(code = "usage.home_failed", error = %e, "couldn't create the provider home");
                }
                let mut env = crate::services::accounts::login_env(
                    &state.terminals,
                    engine,
                    &program,
                    &home,
                    "usage",
                )
                .await
                .unwrap_or_default();
                env.extend(crate::services::fake_engine_env());
                match tandem_engines::usage::codex_rate_limits(&program, &env, &home, PROBE_WAIT)
                    .await
                {
                    Ok(windows) if !windows.is_empty() => {
                        view.windows = windows;
                        view.source = UsageSource::CodexAppServer;
                        view.reason = None;
                    }
                    Ok(_) => view.reason = Some(format!("{label} reported no usage windows.")),
                    Err(e) => {
                        tracing::warn!(code = "usage.probe_failed", error = %e, "couldn't read usage");
                        view.reason = Some(format!("Couldn't read {label}'s usage ({e})."));
                    }
                }
            }
        }
    }
    if !view.windows.is_empty() {
        view.forecasts = state
            .usage
            .record(account.id.as_str(), &view.windows, view.fetched_at);
    }
    state.usage.put(&view);
    state
        .bus
        .publish(BusEvent::new(Topic::Engines, "usage:changed", view.clone()));
    warn_if_short(state, account, &label, &view).await;
    view
}

/// "Warn me before usage runs out" (F-AC-08, off by default): one
/// notification per account, window and reset.
async fn warn_if_short(
    state: &crate::state::AppState,
    account: &tandem_store::repo::accounts::Account,
    engine: &str,
    view: &UsageView,
) {
    let on = tandem_store::repo::settings::get(&state.store, SettingKey::UsageForecastNotify)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !on {
        return;
    }
    let tz = *chrono::Local::now().offset();
    for (window, forecast) in state.usage.newly_short(view) {
        let (Some(at), Some(before)) = (forecast.runs_out_at, forecast.before_reset_ms) else {
            continue;
        };
        let (title, body) = short_text(engine, &account.label, &window.kind, at, before, tz);
        state
            .notifications
            .raise_usage(title, body, account.id.as_str())
            .await;
    }
}

/// The notification's title and body.
pub fn short_text(
    engine: &str,
    account: &str,
    kind: &str,
    at: i64,
    before_ms: i64,
    tz: chrono::FixedOffset,
) -> (String, String) {
    let time = chrono::DateTime::from_timestamp_millis(at)
        .map(|t| t.with_timezone(&tz).format("%-I:%M %p").to_string())
        .unwrap_or_default();
    let window = match kind {
        "5h" => "5-hour window".to_string(),
        "week" => "weekly window".to_string(),
        other => format!("{other} window"),
    };
    (
        format!("{engine} ({account}) runs out at {time}"),
        format!(
            "At this pace the {window} runs out {} before it resets.",
            duration_text(before_ms)
        ),
    )
}

/// "1 h 30 min", "45 min", "2 d 3 h".
pub fn duration_text(ms: i64) -> String {
    let minutes = (ms.max(0) + 30_000) / 60_000;
    let (d, h, m) = (minutes / 1_440, minutes % 1_440 / 60, minutes % 60);
    match (d, h, m) {
        (0, 0, m) => format!("{m} min"),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) => format!("{h} h {m} min"),
        (d, 0, _) => format!("{d} d"),
        (d, h, _) => format!("{d} d {h} h"),
    }
}

/// One session's share of an account's tokens in the last hour (F-AC-08).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionShare {
    pub session_id: String,
    /// The pane running it, if one is.
    pub pane_id: Option<String>,
    pub tokens: u64,
    /// 0–1 of the account's tokens in the hour.
    pub share: f64,
}

/// `usage_sessions` row: an account's sessions in the last hour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountSessions {
    pub account_id: tandem_core::ids::AccountId,
    pub engine_id: String,
    /// The engine keeps logs Vibezeo can read (`usage_log_globs`).
    pub readable: bool,
    pub sessions: Vec<SessionShare>,
}

/// The span "Who's using it" covers.
pub const SESSIONS_SPAN_MS: i64 = 60 * 60_000;

/// Groups per-session tokens by account, with shares and panes.
pub fn shares(
    accounts: &[tandem_store::repo::accounts::Account],
    readable: &dyn Fn(&str) -> bool,
    tokens: &[tandem_engines::usage_log::SessionTokens],
    panes: &[(String, String)],
) -> Vec<AccountSessions> {
    accounts
        .iter()
        .map(|a| {
            let mine: Vec<_> = tokens
                .iter()
                .filter(|t| t.account_id == a.id.as_str())
                .collect();
            let total: u64 = mine.iter().map(|t| t.tokens).sum();
            AccountSessions {
                account_id: a.id.clone(),
                engine_id: a.engine_id.clone(),
                readable: readable(&a.engine_id),
                sessions: mine
                    .iter()
                    .map(|t| SessionShare {
                        session_id: t.session_id.clone(),
                        pane_id: panes
                            .iter()
                            .find(|(_, s)| *s == t.session_id)
                            .map(|(p, _)| p.clone()),
                        tokens: t.tokens,
                        share: if total == 0 {
                            0.0
                        } else {
                            t.tokens as f64 / total as f64
                        },
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod forecast_tests {
    use super::*;
    use tandem_core::ids::AccountId;
    use tandem_engines::forecast::ForecastState;
    use tandem_engines::usage_log::SessionTokens;
    use tandem_store::repo::accounts::Account;

    const MIN: i64 = 60_000;
    const T0: i64 = 1_790_000_000_000;

    fn account(id: &str, engine: &str) -> Account {
        Account {
            id: AccountId::from_db(id.into()),
            engine_id: engine.into(),
            label: "Work".into(),
            home_path: "/tmp/h".into(),
            is_default: true,
            plan_label: None,
            created_at: 0,
        }
    }

    fn view(board: &UsageBoard, used: f64, at: i64, reset: i64) -> UsageView {
        let windows = vec![UsageWindow {
            kind: "5h".into(),
            used_pct: used,
            resets_at: Some(reset),
        }];
        UsageView {
            account_id: AccountId::from_db("acc_1".into()),
            engine_id: "codex".into(),
            forecasts: board.record("acc_1", &windows, at),
            windows,
            source: UsageSource::CodexAppServer,
            estimated: false,
            reason: None,
            fetched_at: at,
        }
    }

    #[test]
    fn f_ac_08_5_a_short_window_is_warned_about_once_per_reset() {
        let board = UsageBoard::default();
        let reset = T0 + 120 * MIN;
        let first = view(&board, 40.0, T0, reset);
        assert!(board.newly_short(&first).is_empty(), "not enough data yet");
        let second = view(&board, 60.0, T0 + 30 * MIN, reset);
        assert_eq!(second.forecasts[0].state, ForecastState::RunsOut);
        assert_eq!(board.newly_short(&second).len(), 1);
        let third = view(&board, 65.0, T0 + 35 * MIN, reset);
        assert!(board.newly_short(&third).is_empty(), "same window: once");
        // Engines' reset times drift by seconds between readings.
        let drift = view(&board, 70.0, T0 + 40 * MIN, reset + 30_000);
        assert!(
            board.newly_short(&drift).is_empty(),
            "a drifted reset is the same window"
        );
        // A new window (a later reset) can warn again.
        board.forget("acc_1");
        let later = T0 + 400 * MIN;
        view(&board, 10.0, T0 + 300 * MIN, later);
        let again = view(&board, 50.0, T0 + 330 * MIN, later);
        assert_eq!(board.newly_short(&again).len(), 1);
    }

    #[test]
    fn f_ac_08_5_the_notification_text_names_the_time_and_the_margin() {
        let utc = chrono::FixedOffset::east_opt(0).expect("utc");
        // 2026-10-15 15:40 UTC.
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-15T15:40:00Z")
            .expect("time")
            .timestamp_millis();
        let (title, body) = short_text("Codex", "Work", "5h", at, 90 * MIN, utc);
        assert_eq!(title, "Codex (Work) runs out at 3:40 PM");
        assert_eq!(
            body,
            "At this pace the 5-hour window runs out 1 h 30 min before it resets."
        );
        assert_eq!(duration_text(45 * MIN), "45 min");
        assert_eq!(duration_text(120 * MIN), "2 h");
        assert_eq!(duration_text(51 * 60 * MIN), "2 d 3 h");
    }

    #[test]
    fn f_ac_08_4_shares_name_the_pane_running_the_session() {
        let accounts = [account("acc_1", "claude"), account("acc_2", "gemini")];
        let tokens = [
            SessionTokens {
                engine_id: "claude".into(),
                account_id: "acc_1".into(),
                session_id: "ses-a".into(),
                tokens: 3_000,
            },
            SessionTokens {
                engine_id: "claude".into(),
                account_id: "acc_1".into(),
                session_id: "ses-b".into(),
                tokens: 1_000,
            },
        ];
        let panes = [("pn_9".to_string(), "ses-a".to_string())];
        let readable = |engine: &str| engine == "claude";
        let out = shares(&accounts, &readable, &tokens, &panes);
        assert_eq!(out.len(), 2);
        let a = &out[0];
        assert!(a.readable);
        let got: Vec<(Option<&str>, f64)> = a
            .sessions
            .iter()
            .map(|s| (s.pane_id.as_deref(), s.share))
            .collect();
        assert_eq!(got, vec![(Some("pn_9"), 0.75), (None, 0.25)]);
        assert!(!out[1].readable);
        assert!(out[1].sessions.is_empty());
    }
}
