//! Runaway guard (F-AC-09): watches agent panes (and threads, for time)
//! through the status bus, trips the limits in `tandem_engines::guard`,
//! shows a card on the pane (UI-170), raises a `runaway` notification, and
//! pauses the agent when the user asks (or chose "Pause the agent"): the
//! engine's interrupt key first, then, if it's still working 5 s later, its
//! foreground process group is stopped through [`ProcessControl`].
//!
//! [`Guardian`] holds the decisions and is tested on its own; the service
//! glues it to the app's status, terminals, usage and settings.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::settings::SettingKey;
use tandem_core::source::SourceRef;
use tandem_core::status::{AgentStatus, StatusRecord};
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::guard::{GuardLimits, PaneGuard, Trip};
use tandem_pty::suspend::{PausedJob, ProcessControl};
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use crate::state::AppState;

/// How often the guard looks at every agent.
pub const TICK: Duration = Duration::from_secs(5);
/// Window points are recomputed every this many ticks (5 minutes).
const POINTS_EVERY: u64 = 60;
/// After the interrupt key, how long the agent has to stop by itself.
pub const PAUSE_GRACE: Duration = Duration::from_secs(5);

/// The card's state (UI-170).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GuardCardState {
    /// A limit tripped: Pause and Dismiss.
    Tripped,
    /// The interrupt key went out; waiting to see if it stopped.
    Pausing,
    /// The agent stopped its turn after the interrupt key.
    Interrupted,
    /// Its process group is stopped: Resume.
    Paused,
    /// Couldn't pause (Windows, or the program is gone): the reason.
    CantPause,
}

/// UI-170, one per pane with a tripped limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GuardCard {
    pub pane_id: String,
    pub trip: Trip,
    pub state: GuardCardState,
    /// The card's headline ("fix-auth has been working for 90 min").
    pub title: String,
    /// Why pausing failed.
    pub reason: Option<String>,
}

/// `runaway:changed`: a pane's card, or `null` once it's gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GuardCardChange {
    pub pane_id: String,
    pub card: Option<GuardCard>,
}

/// "Notify me" or "Pause the agent" (setting `runaway_action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardAction {
    Notify,
    Pause,
}

impl GuardAction {
    pub fn from_json(v: &serde_json::Value) -> Self {
        if v.as_str() == Some("pause") {
            Self::Pause
        } else {
            Self::Notify
        }
    }
}

#[derive(Debug, Default)]
struct Entry {
    guard: PaneGuard,
    last: Option<AgentStatus>,
    /// The pane's engine, once known (subagent tools, interrupt key).
    engine: Option<String>,
}

/// A limit that tripped, for which session.
#[derive(Debug, Clone, PartialEq)]
pub struct Tripped {
    /// The status key (`pane:…` or `thread:…`).
    pub key: String,
    pub trip: Trip,
}

impl Tripped {
    pub fn pane(&self) -> Option<&str> {
        self.key.strip_prefix("pane:")
    }
}

/// The guard's decisions over status snapshots.
#[derive(Debug, Default)]
pub struct Guardian {
    entries: HashMap<String, Entry>,
}

fn watched(key: &str) -> bool {
    key.starts_with("pane:") || key.starts_with("thread:")
}

impl Guardian {
    /// One look at every session: new turns, ended turns (with `screen` for
    /// a pane's ending), time working and window points (`points` per pane).
    pub fn observe(
        &mut self,
        records: &[StatusRecord],
        now: i64,
        limits: &GuardLimits,
        points: &HashMap<String, f64>,
        screen: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<Tripped> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for rec in records.iter().filter(|r| watched(&r.key.0)) {
            let key = rec.key.0.clone();
            seen.push(key.clone());
            let pane = key.strip_prefix("pane:").map(str::to_string);
            let entry = self.entries.entry(key.clone()).or_default();
            let prev = entry.last.replace(rec.status);
            let was_working = prev == Some(AgentStatus::Working);
            match rec.status {
                AgentStatus::Working => {
                    // A new turn starts from done or idle (or the first look);
                    // back from waiting on an approval is the same turn.
                    if !was_working && prev != Some(AgentStatus::Waiting) {
                        entry.guard.prompt(rec.since);
                    }
                    entry.guard.working(rec.since);
                }
                AgentStatus::Done if was_working => {
                    // Only panes compare turn endings (threads have no screen).
                    let hash = pane
                        .as_deref()
                        .and_then(screen)
                        .and_then(|s| tandem_engines::guard::screen_hash(&s));
                    let limits = if pane.is_some() {
                        *limits
                    } else {
                        GuardLimits {
                            circles: false,
                            ..*limits
                        }
                    };
                    if let Some(trip) = entry.guard.turn_end(hash, &limits) {
                        out.push(Tripped {
                            key: key.clone(),
                            trip,
                        });
                    }
                }
                _ => entry.guard.stopped(),
            }
            let pts = pane.as_deref().and_then(|p| points.get(p)).copied();
            for trip in entry.guard.tick(now, pts, limits) {
                out.push(Tripped {
                    key: key.clone(),
                    trip,
                });
            }
        }
        self.entries.retain(|k, _| seen.contains(k));
        out
    }

    /// A tool report from a pane: counts it when it starts a subagent.
    pub fn tool(&mut self, pane: &str, subagent: bool, limits: &GuardLimits) -> Option<Tripped> {
        let key = format!("pane:{pane}");
        // A tool report can come before the guard's first look: the agent
        // is working, so the first look doesn't start a new turn.
        let entry = self.entries.entry(key.clone()).or_insert_with(|| Entry {
            last: Some(AgentStatus::Working),
            ..Entry::default()
        });
        entry
            .guard
            .tool(subagent, limits)
            .map(|trip| Tripped { key, trip })
    }

    /// Keeps a pane's limit quiet until its next prompt.
    pub fn dismiss(&mut self, pane: &str, kind: &str) {
        if let Some(e) = self.entries.get_mut(&format!("pane:{pane}")) {
            e.guard.dismiss(kind);
        }
    }

    pub fn engine(&self, pane: &str) -> Option<String> {
        self.entries.get(&format!("pane:{pane}"))?.engine.clone()
    }

    /// Panes whose engine isn't known yet.
    pub fn unknown_engines(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, e)| e.engine.is_none())
            .filter_map(|(k, _)| k.strip_prefix("pane:").map(str::to_string))
            .collect()
    }

    pub fn set_engine(&mut self, pane: &str, engine: String) {
        if let Some(e) = self.entries.get_mut(&format!("pane:{pane}")) {
            e.engine = Some(engine);
        }
    }
}

/// The trip's wording, after the agent's name.
pub fn trip_title(name: &str, trip: &Trip, account: Option<&str>) -> String {
    match trip {
        Trip::Working { minutes } => format!("{name} has been working for {minutes} min"),
        Trip::Window { points } => match account {
            Some(a) => format!("{name} used {points:.0} points of {a}'s window in the last hour"),
            None => {
                format!("{name} used {points:.0} points of its account's window in the last hour")
            }
        },
        Trip::Subagents { count } => {
            format!("{name} started {count} subagents since your last prompt")
        }
        Trip::Circles { turns } => {
            format!("{name} may be going in circles: its last {turns} turns ended the same way")
        }
    }
}

fn trip_kind(trip: &Trip) -> &'static str {
    match trip {
        Trip::Working { .. } => "working",
        Trip::Window { .. } => "window",
        Trip::Subagents { .. } => "subagents",
        Trip::Circles { .. } => "circles",
    }
}

pub struct RunawayService {
    bus: EventBus,
    guardian: Mutex<Guardian>,
    cards: Mutex<HashMap<String, GuardCard>>,
    /// Stopped panes' process groups, for Resume.
    stopped: Mutex<HashMap<String, PausedJob>>,
    control: Arc<dyn ProcessControl>,
}

impl RunawayService {
    pub fn new(bus: EventBus, control: Arc<dyn ProcessControl>) -> Arc<Self> {
        Arc::new(Self {
            bus,
            guardian: Mutex::default(),
            cards: Mutex::default(),
            stopped: Mutex::default(),
            control,
        })
    }

    pub fn list(&self) -> Vec<GuardCard> {
        self.cards
            .lock()
            .map(|c| c.values().cloned().collect())
            .unwrap_or_default()
    }

    fn set_card(&self, pane: &str, card: Option<GuardCard>) {
        if let Ok(mut cards) = self.cards.lock() {
            match &card {
                Some(c) => {
                    cards.insert(pane.to_string(), c.clone());
                }
                None => {
                    cards.remove(pane);
                }
            }
        }
        self.bus.publish(BusEvent::new(
            Topic::Status,
            "runaway:changed",
            GuardCardChange {
                pane_id: pane.to_string(),
                card,
            },
        ));
    }

    fn card(&self, pane: &str) -> Option<GuardCard> {
        self.cards.lock().ok()?.get(pane).cloned()
    }

    fn update(&self, pane: &str, state: GuardCardState, reason: Option<String>) {
        if let Some(mut card) = self.card(pane) {
            card.state = state;
            card.reason = reason;
            self.set_card(pane, Some(card));
        }
    }

    /// Dismiss: the card goes, and its limit stays quiet until the next
    /// prompt. A stopped agent is continued first.
    pub fn dismiss(&self, pane: &str) {
        if let Some(card) = self.card(pane) {
            if let Ok(mut g) = self.guardian.lock() {
                g.dismiss(pane, trip_kind(&card.trip));
            }
        }
        self.resume_group(pane);
        self.set_card(pane, None);
    }

    fn resume_group(&self, pane: &str) -> Option<Result<(), tandem_pty::suspend::SuspendError>> {
        let job = self.stopped.lock().ok()?.remove(pane)?;
        Some(tandem_pty::suspend::resume(self.control.as_ref(), &job))
    }

    /// Resume: continues a stopped agent; the card goes.
    pub fn resume(&self, pane: &str) -> Result<(), IpcError> {
        match self.resume_group(pane) {
            Some(Err(e)) => Err(IpcError::new(ErrorCode::Io, e.to_string())),
            _ => {
                self.set_card(pane, None);
                Ok(())
            }
        }
    }

    /// Pause: the interrupt key, then (still working after the grace) a
    /// stop of the pane's foreground process group. Never types a prompt,
    /// and never changes the pane's account or engine.
    pub async fn pause(&self, state: &AppState, pane: &str) -> Result<(), IpcError> {
        if self.card(pane).is_none() {
            return Err(IpcError::new(ErrorCode::NotFound, "Nothing to pause."));
        }
        self.update(pane, GuardCardState::Pausing, None);
        let engine = self
            .guardian
            .lock()
            .ok()
            .and_then(|g| g.engine(pane))
            .and_then(|id| state.terminals.engines.get(&id));
        let key = engine
            .as_ref()
            .and_then(|e| e.interrupt.clone())
            .unwrap_or_else(|| "\u{1b}".to_string());
        if let Err(e) = state.terminals.pty.write(pane, key.as_bytes()) {
            self.update(pane, GuardCardState::CantPause, Some(e.to_string()));
            return Ok(());
        }
        state.clock.sleep(PAUSE_GRACE).await;
        let working = state
            .terminals
            .status
            .snapshot()
            .iter()
            .any(|r| r.key.0 == format!("pane:{pane}") && r.status == AgentStatus::Working);
        let kind = workspace_kind(state, pane).await;
        let group = if working && kind.as_deref() == Some("local") {
            state.terminals.pty.foreground_group(pane)
        } else {
            None
        };
        match after_grace(working, kind.as_deref(), group) {
            AfterGrace::Interrupted => self.update(pane, GuardCardState::Interrupted, None),
            AfterGrace::CantPause(reason) => {
                self.update(pane, GuardCardState::CantPause, Some(reason.into()))
            }
            AfterGrace::Stop(pgid) => {
                match state
                    .terminals
                    .pty
                    .pause_job(pane, pgid, self.control.as_ref())
                {
                    Ok(job) => {
                        if let Ok(mut s) = self.stopped.lock() {
                            s.insert(pane.to_string(), job);
                        }
                        self.update(pane, GuardCardState::Paused, None);
                    }
                    Err(e) => {
                        tracing::warn!(code = "runaway.pause_failed", error = %e, "couldn't pause an agent");
                        self.update(pane, GuardCardState::CantPause, Some(CANT_PAUSE.into()));
                    }
                }
            }
        }
        Ok(())
    }

    /// A tool report (the status bus's sink).
    pub async fn on_tool(
        &self,
        state: &AppState,
        pane: &str,
        tool: &str,
        limits: &GuardLimits,
    ) -> Option<Tripped> {
        let known = self.guardian.lock().ok().and_then(|g| g.engine(pane));
        let engine = match known {
            Some(e) => e,
            None => {
                let id = tandem_core::ids::PaneId::from_db(pane.to_string());
                tandem_store::repo::sessions::latest_for_pane(&state.store, &id)
                    .await
                    .ok()??
                    .engine_id
            }
        };
        let spec = state.terminals.engines.get(&engine)?;
        let subagent = spec.subagent_tools.iter().any(|t| t == tool);
        let mut g = self.guardian.lock().ok()?;
        let tripped = g.tool(pane, subagent, limits);
        g.set_engine(pane, engine);
        tripped
    }

    /// Starts the guard's loop and its tool sink.
    pub fn start(self: &Arc<Self>, app: AppHandle) {
        let me = self.clone();
        let handle = app.clone();
        let sink_state = app.clone();
        let sink_me = self.clone();
        app.state::<AppState>()
            .terminals
            .status
            .set_tool_sink(Arc::new(move |pane: &str, tool: &str| {
                let me = sink_me.clone();
                let app = sink_state.clone();
                let (pane, tool) = (pane.to_string(), tool.to_string());
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let (limits, action) = settings(&state).await;
                    if let Some(t) = me.on_tool(&state, &pane, &tool, &limits).await {
                        me.tripped(&state, t, action).await;
                    }
                });
            }));
        tauri::async_runtime::spawn(async move {
            let mut n: u64 = 0;
            loop {
                let state = handle.state::<AppState>();
                me.tick(&state, n.is_multiple_of(POINTS_EVERY)).await;
                n += 1;
                state.clock.sleep(TICK).await;
            }
        });
    }

    async fn tick(&self, state: &AppState, with_points: bool) {
        let (limits, action) = settings(state).await;
        // Learn new panes' engines (once each).
        let unknown = self
            .guardian
            .lock()
            .map(|g| g.unknown_engines())
            .unwrap_or_default();
        for pane in unknown {
            let id = tandem_core::ids::PaneId::from_db(pane.clone());
            if let Ok(Some(s)) =
                tandem_store::repo::sessions::latest_for_pane(&state.store, &id).await
            {
                if let Ok(mut g) = self.guardian.lock() {
                    g.set_engine(&pane, s.engine_id);
                }
            }
        }
        let points = if with_points && limits.window_points.is_some() {
            window_points(state).await
        } else {
            HashMap::new()
        };
        let records = state.terminals.status.snapshot();
        let now = state.clock.now_ms();
        let pty = &state.terminals.pty;
        let screen = |p: &str| pty.screen_text(p);
        let trips = self
            .guardian
            .lock()
            .map(|mut g| g.observe(&records, now, &limits, &points, &screen))
            .unwrap_or_default();
        for t in trips {
            self.tripped(state, t, action).await;
        }
    }

    /// A tripped limit: the card (panes), the notification, and the pause
    /// when the user chose it.
    async fn tripped(&self, state: &AppState, t: Tripped, action: GuardAction) {
        let records = state.terminals.status.snapshot();
        let Some(rec) = records.iter().find(|r| r.key.0 == t.key).cloned() else {
            return;
        };
        let name = match crate::services::dashboard::item_for(state, rec).await {
            Some(item) => crate::services::notifications::display_name(&item),
            None => "An agent".to_string(),
        };
        let account = match &t.trip {
            Trip::Window { .. } => account_label(state, t.pane()).await,
            _ => None,
        };
        let title = trip_title(&name, &t.trip, account.as_deref());
        let (source, body) = match t.pane() {
            Some(pane) => {
                // A paused or pausing card isn't replaced by another limit.
                let busy = self.card(pane).is_some_and(|c| {
                    matches!(c.state, GuardCardState::Pausing | GuardCardState::Paused)
                });
                if !busy {
                    self.set_card(
                        pane,
                        Some(GuardCard {
                            pane_id: pane.to_string(),
                            trip: t.trip.clone(),
                            state: GuardCardState::Tripped,
                            title: title.clone(),
                            reason: None,
                        }),
                    );
                }
                (
                    SourceRef::Pane {
                        pane_id: pane.to_string(),
                    },
                    "Runaway guard: pause it, or dismiss to let it carry on.".to_string(),
                )
            }
            None => (
                SourceRef::Thread {
                    thread_id: t.key.trim_start_matches("thread:").to_string(),
                },
                "Runaway guard: open it to stop or steer it.".to_string(),
            ),
        };
        state.notifications.raise_runaway(title, body, source).await;
        if action == GuardAction::Pause {
            if let Some(pane) = t.pane() {
                if let Err(e) = self.pause(state, pane).await {
                    tracing::warn!(code = "runaway.pause_failed", error = %e, "couldn't pause an agent");
                }
            }
        }
    }
}

const CANT_PAUSE: &str = "Couldn't pause: send it a message or stop it.";
const CANT_PAUSE_SSH: &str =
    "Couldn't pause an agent on an SSH host: send it a message or stop it.";

/// What Pause does once the interrupt key's grace is over.
#[derive(Debug, PartialEq, Eq)]
enum AfterGrace {
    /// The agent stopped its turn by itself.
    Interrupted,
    /// Stop this process group (Linux and macOS).
    Stop(i32),
    /// Nothing on this computer can pause it.
    CantPause(&'static str),
}

/// Only a local pane's foreground group is the agent itself. An SSH pane's
/// local process is `ssh`: stopping it would pause the connection, not the
/// agent on the host, which keeps working (and spending) while the card
/// says Paused, and a long stop can lose the session (D-855). WSL panes
/// run through `wsl.exe` on Windows, where there's no stop either.
fn after_grace(working: bool, workspace_kind: Option<&str>, group: Option<i32>) -> AfterGrace {
    if !working {
        return AfterGrace::Interrupted;
    }
    match (workspace_kind, group) {
        (Some("local"), Some(pgid)) => AfterGrace::Stop(pgid),
        (Some("ssh"), _) => AfterGrace::CantPause(CANT_PAUSE_SSH),
        _ => AfterGrace::CantPause(CANT_PAUSE),
    }
}

/// The kind of the pane's workspace (`local`, `ssh` or `wsl`); `None` when
/// it can't be read, which never pauses by signal.
async fn workspace_kind(state: &AppState, pane: &str) -> Option<String> {
    let id = tandem_core::ids::PaneId::from_db(pane.to_string());
    let rec = tandem_store::repo::panes::get(&state.store, &id)
        .await
        .ok()?;
    tandem_store::repo::workspaces::get(&state.store, &rec.workspace_id)
        .await
        .ok()
        .map(|ws| ws.kind)
}

async fn settings(state: &AppState) -> (GuardLimits, GuardAction) {
    use tandem_store::repo::settings;
    let limits = settings::get(&state.store, SettingKey::RunawayGuard)
        .await
        .map(|v| GuardLimits::from_json(&v))
        .unwrap_or_default();
    let action = settings::get(&state.store, SettingKey::RunawayAction)
        .await
        .map(|v| GuardAction::from_json(&v))
        .unwrap_or(GuardAction::Notify);
    (limits, action)
}

async fn account_label(state: &AppState, pane: Option<&str>) -> Option<String> {
    let pane = tandem_core::ids::PaneId::from_db(pane?.to_string());
    let session = tandem_store::repo::sessions::latest_for_pane(&state.store, &pane)
        .await
        .ok()??;
    let id = session.account_id?;
    let accounts = tandem_store::repo::accounts::list(&state.store)
        .await
        .ok()?;
    let a = accounts.into_iter().find(|a| a.id == id)?;
    let engine = state
        .terminals
        .engines
        .get(&a.engine_id)
        .map(|e| e.label.clone())
        .unwrap_or(a.engine_id);
    Some(format!("{engine} ({})", a.label))
}

/// Each pane's points of its account's window in the last hour: its share
/// of the account's tokens (F-AC-08) times the fullest window's rise.
async fn window_points(state: &AppState) -> HashMap<String, f64> {
    let now = state.clock.now_ms();
    let Ok(accounts) = tandem_store::repo::accounts::list(&state.store).await else {
        return HashMap::new();
    };
    let rises: HashMap<String, f64> = accounts
        .iter()
        .filter_map(|a| {
            state
                .usage
                .hour_rise(a.id.as_str(), now)
                .map(|r| (a.id.as_str().to_string(), r))
        })
        .filter(|(_, r)| *r > 0.0)
        .collect();
    if rises.is_empty() {
        return HashMap::new();
    }
    let mine: Vec<_> = accounts
        .into_iter()
        .filter(|a| rises.contains_key(a.id.as_str()))
        .collect();
    let registry = state.terminals.engines.base_registry();
    let panes = state.terminals.status.engine_sessions();
    let since = now - crate::services::usage::SESSIONS_SPAN_MS;
    let shares = tokio::task::spawn_blocking(move || {
        let sources = crate::services::usage_report::sources(&mine, &registry);
        let tokens = tandem_engines::usage_log::recent_sessions(&sources, since);
        let readable = |_: &str| true;
        crate::services::usage::shares(&mine, &readable, &tokens, &panes)
    })
    .await
    .unwrap_or_default();
    let mut out = HashMap::new();
    for account in shares {
        let Some(rise) = rises.get(account.account_id.as_str()) else {
            continue;
        };
        for s in account.sessions {
            if let Some(pane) = s.pane_id {
                out.insert(pane, tandem_engines::guard::window_points(s.share, *rise));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::status::SessionKey;

    const MIN: i64 = 60_000;

    fn rec(key: &str, status: AgentStatus, since: i64) -> StatusRecord {
        StatusRecord {
            key: SessionKey(key.into()),
            status,
            reason: None,
            since,
            ts: since,
            tool: None,
            title: None,
            preview: None,
        }
    }

    fn no_screen(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn f_ac_09_7_only_a_local_agent_is_stopped_after_the_grace() {
        // Stopped its turn by itself: nothing else happens, wherever it runs.
        assert_eq!(
            after_grace(false, Some("ssh"), None),
            AfterGrace::Interrupted
        );
        assert_eq!(
            after_grace(false, Some("local"), Some(42)),
            AfterGrace::Interrupted
        );
        // Still working: a local agent's group is stopped.
        assert_eq!(
            after_grace(true, Some("local"), Some(42)),
            AfterGrace::Stop(42)
        );
        // On an SSH host the local process is the connection: never signalled.
        assert_eq!(
            after_grace(true, Some("ssh"), Some(42)),
            AfterGrace::CantPause(CANT_PAUSE_SSH)
        );
        // WSL, a pane with no foreground group, or an unreadable workspace.
        for (kind, group) in [
            (Some("wsl"), Some(42)),
            (Some("local"), None),
            (None, Some(42)),
        ] {
            assert_eq!(
                after_grace(true, kind, group),
                AfterGrace::CantPause(CANT_PAUSE)
            );
        }
    }

    #[test]
    fn f_ac_09_1_a_pane_working_past_the_limit_trips_once() {
        let mut g = Guardian::default();
        let limits = GuardLimits::default();
        let none = HashMap::new();
        let r = [rec("pane:pn_1", AgentStatus::Working, 0)];
        assert!(g.observe(&r, MIN, &limits, &none, &no_screen).is_empty());
        let t = g.observe(&r, 91 * MIN, &limits, &none, &no_screen);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].pane(), Some("pn_1"));
        assert_eq!(t[0].trip, Trip::Working { minutes: 90 });
        assert!(g
            .observe(&r, 95 * MIN, &limits, &none, &no_screen)
            .is_empty());
        // Threads trip on time too, without a pane.
        let th = [rec("thread:th_1", AgentStatus::Working, 0)];
        let t = g.observe(&th, 91 * MIN, &limits, &none, &no_screen);
        assert_eq!(t[0].pane(), None);
        assert_eq!(
            trip_title("fix-auth", &Trip::Working { minutes: 90 }, None),
            "fix-auth has been working for 90 min"
        );
    }

    #[test]
    fn f_ac_09_3_three_turns_ending_the_same_way_trip_circles() {
        let mut g = Guardian::default();
        let limits = GuardLimits::default();
        let none = HashMap::new();
        let same = |_: &str| Some("Error: test failed\n> retrying".to_string());
        let mut trips = Vec::new();
        for turn in 0..3 {
            let t0 = turn * 10 * MIN;
            g.observe(
                &[rec("pane:pn_1", AgentStatus::Working, t0)],
                t0 + MIN,
                &limits,
                &none,
                &same,
            );
            trips.extend(g.observe(
                &[rec("pane:pn_1", AgentStatus::Done, t0 + 2 * MIN)],
                t0 + 2 * MIN,
                &limits,
                &none,
                &same,
            ));
        }
        assert_eq!(trips.len(), 1);
        assert_eq!(trips[0].trip, Trip::Circles { turns: 3 });
        // Threads never compare endings.
        let mut g = Guardian::default();
        let mut trips = Vec::new();
        for turn in 0..3 {
            let t0 = turn * 10 * MIN;
            g.observe(
                &[rec("thread:t", AgentStatus::Working, t0)],
                t0,
                &limits,
                &none,
                &same,
            );
            trips.extend(g.observe(
                &[rec("thread:t", AgentStatus::Done, t0 + MIN)],
                t0 + MIN,
                &limits,
                &none,
                &same,
            ));
        }
        assert!(trips.is_empty());
    }

    #[test]
    fn f_ac_09_2_subagent_reports_count_toward_the_limit() {
        let mut g = Guardian::default();
        let limits = GuardLimits {
            subagents: Some(2),
            ..GuardLimits::default()
        };
        let none = HashMap::new();
        g.observe(
            &[rec("pane:pn_1", AgentStatus::Working, 0)],
            0,
            &limits,
            &none,
            &no_screen,
        );
        assert_eq!(g.tool("pn_1", true, &limits), None);
        assert_eq!(g.tool("pn_1", false, &limits), None);
        let t = g.tool("pn_1", true, &limits).expect("tripped");
        assert_eq!(t.trip, Trip::Subagents { count: 2 });
        assert_eq!(g.tool("pn_9", true, &limits), None, "unknown pane");
    }

    #[test]
    fn f_ac_09_4_window_points_from_the_pane_trip_the_window_limit() {
        let mut g = Guardian::default();
        let limits = GuardLimits::default();
        let pts = HashMap::from([("pn_1".to_string(), 34.0)]);
        let t = g.observe(
            &[rec("pane:pn_1", AgentStatus::Working, 0)],
            MIN,
            &limits,
            &pts,
            &no_screen,
        );
        assert_eq!(t[0].trip, Trip::Window { points: 34.0 });
        assert_eq!(
            trip_title("fix-auth", &t[0].trip, Some("Codex (Work)")),
            "fix-auth used 34 points of Codex (Work)'s window in the last hour"
        );
    }

    #[test]
    fn f_ac_09_5_dismiss_keeps_the_limit_quiet_until_the_next_prompt() {
        let mut g = Guardian::default();
        let limits = GuardLimits::default();
        let none = HashMap::new();
        g.observe(
            &[rec("pane:pn_1", AgentStatus::Working, 0)],
            0,
            &limits,
            &none,
            &no_screen,
        );
        g.dismiss("pn_1", "working");
        assert!(g
            .observe(
                &[rec("pane:pn_1", AgentStatus::Working, 0)],
                100 * MIN,
                &limits,
                &none,
                &no_screen
            )
            .is_empty());
        // Done, then a new turn: the limit can trip again.
        g.observe(
            &[rec("pane:pn_1", AgentStatus::Done, 101 * MIN)],
            101 * MIN,
            &limits,
            &none,
            &no_screen,
        );
        g.observe(
            &[rec("pane:pn_1", AgentStatus::Working, 102 * MIN)],
            102 * MIN,
            &limits,
            &none,
            &no_screen,
        );
        let t = g.observe(
            &[rec("pane:pn_1", AgentStatus::Working, 102 * MIN)],
            193 * MIN,
            &limits,
            &none,
            &no_screen,
        );
        assert_eq!(t.len(), 1);
        // Back from waiting on an approval is the same turn: no new prompt.
        let mut g = Guardian::default();
        g.observe(
            &[rec("pane:pn_2", AgentStatus::Working, 0)],
            0,
            &limits,
            &none,
            &no_screen,
        );
        g.dismiss("pn_2", "working");
        g.observe(
            &[rec("pane:pn_2", AgentStatus::Waiting, MIN)],
            MIN,
            &limits,
            &none,
            &no_screen,
        );
        assert!(g
            .observe(
                &[rec("pane:pn_2", AgentStatus::Working, 2 * MIN)],
                200 * MIN,
                &limits,
                &none,
                &no_screen
            )
            .is_empty());
    }

    #[derive(Default)]
    struct FakeControl {
        calls: Mutex<Vec<(&'static str, i32)>>,
    }

    impl ProcessControl for FakeControl {
        fn stop(&self, pgid: i32) -> Result<(), tandem_pty::suspend::SuspendError> {
            if let Ok(mut c) = self.calls.lock() {
                c.push(("stop", pgid));
            }
            Ok(())
        }
        fn cont(&self, pgid: i32) -> Result<(), tandem_pty::suspend::SuspendError> {
            if let Ok(mut c) = self.calls.lock() {
                c.push(("cont", pgid));
            }
            Ok(())
        }
    }

    #[test]
    fn f_ac_09_6_dismissing_a_paused_agent_continues_it_and_resume_clears_the_card() {
        let control = Arc::new(FakeControl::default());
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::Status);
        let svc = RunawayService::new(bus, control.clone());
        let card = GuardCard {
            pane_id: "pn_1".into(),
            trip: Trip::Working { minutes: 90 },
            state: GuardCardState::Paused,
            title: "x".into(),
            reason: None,
        };
        svc.set_card("pn_1", Some(card.clone()));
        svc.stopped.lock().expect("lock").insert(
            "pn_1".into(),
            PausedJob {
                agent: 4242,
                shell: Some(4240),
            },
        );
        svc.dismiss("pn_1");
        // The agent first, then its shell (D-857).
        assert_eq!(
            control.calls.lock().expect("lock").as_slice(),
            &[("cont", 4242), ("cont", 4240)]
        );
        assert!(svc.list().is_empty());
        assert_eq!(rx.try_recv().expect("event").name, "runaway:changed");
        // Resume with nothing stopped just clears the card.
        svc.set_card("pn_1", Some(card));
        svc.resume("pn_1").expect("resume");
        assert!(svc.list().is_empty());
        assert_eq!(
            control.calls.lock().expect("lock").len(),
            2,
            "no stray signal"
        );
    }
}
