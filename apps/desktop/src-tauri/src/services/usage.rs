//! Context fill per agent pane (F-AC-06), read from the engine's own session
//! transcript when a turn ends, and usage windows per account (F-AC-04),
//! from the engine where it reports them.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_engines::transcript::ContextFill;
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
    pub windows: Vec<tandem_engines::usage::UsageWindow>,
    pub source: UsageSource,
    /// From local logs rather than the engine (none yet).
    pub estimated: bool,
    /// Why there's no data ("Usage unavailable" tooltip).
    pub reason: Option<String>,
    pub fetched_at: i64,
}

/// The latest usage per account.
#[derive(Default)]
pub struct UsageBoard {
    views: Mutex<HashMap<String, UsageView>>,
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
    state.usage.put(&view);
    state
        .bus
        .publish(BusEvent::new(Topic::Engines, "usage:changed", view.clone()));
    view
}
