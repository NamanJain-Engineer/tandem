//! Agent status for panes (ARCHITECTURE §6.6, F-EN-05): signed hook reports
//! from the status server, and output heuristics for engines without hooks,
//! into the one [`StatusStore`]. Changes reach the webview as
//! `status:changed`, coalesced over 50 ms.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::status::{SessionKey, StatusRecord, StatusStore, StatusUpdate};
use tandem_core::status_heuristics::Heuristics;
use tandem_core::status_report::{ReportEvent, StatusReport};
use tandem_engines::status_server::{ReportSink, StatusServer};
use tokio::sync::OnceCell;

/// `status:changed` is sent at most this often.
pub const COALESCE: Duration = Duration::from_millis(50);
/// How often quiet panes are checked for done and idle.
pub const TICK: Duration = Duration::from_secs(1);
/// Screen text is checked for waiting patterns at most this often per pane.
const SCREEN_CHECK_MS: i64 = 300;

/// A non-pane source's card details.
#[derive(Debug, Clone)]
pub struct ExternalMeta {
    pub source: tandem_core::source::SourceRef,
    pub title: String,
    pub engine_id: Option<String>,
    pub workspace_id: Option<String>,
}

struct Watched {
    heuristics: Heuristics,
    last_screen_check: Option<i64>,
}

pub struct StatusService {
    pub store: Arc<StatusStore>,
    server: OnceCell<StatusServer>,
    watched: Mutex<HashMap<String, Watched>>,
    pending: Mutex<Vec<StatusRecord>>,
    /// Keys whose record went away (the agent left), sent with the next batch.
    removed: Mutex<Vec<String>>,
    flushing: Mutex<bool>,
    /// The engine session id each pane's hooks reported (resume, F-CD-03).
    sessions: Mutex<HashMap<String, String>>,
    /// Last output per watched pane (ms).
    activity: Mutex<HashMap<String, i64>>,
    /// Sources that aren't panes (test fixtures now, threads later): what
    /// their Dashboard card shows.
    external: Mutex<HashMap<String, ExternalMeta>>,
    /// Told when a pane's engine reports a new session id (it's persisted at
    /// once so a crash can resume it, F-SH-07).
    on_session: std::sync::OnceLock<SessionSink>,
    bus: EventBus,
    clock: Arc<dyn Clock>,
}

/// `(pane_id, engine_session_id)`.
pub type SessionSink = Arc<dyn Fn(&str, &str) + Send + Sync>;

impl StatusService {
    pub fn new(bus: EventBus, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(StatusStore::new()),
            server: OnceCell::new(),
            watched: Mutex::default(),
            pending: Mutex::default(),
            removed: Mutex::default(),
            flushing: Mutex::new(false),
            sessions: Mutex::default(),
            activity: Mutex::default(),
            external: Mutex::default(),
            on_session: std::sync::OnceLock::new(),
            bus,
            clock,
        })
    }

    /// Where reported engine session ids go (set once, at startup).
    pub fn set_session_sink(&self, sink: SessionSink) {
        let _ = self.on_session.set(sink);
    }

    /// Starts the status server in `run_dir` and the done/idle ticker.
    pub async fn start(self: &Arc<Self>, run_dir: &std::path::Path) {
        let me = Arc::downgrade(self);
        let sink: Arc<dyn ReportSink> = Arc::new(move |report: StatusReport| {
            if let Some(me) = me.upgrade() {
                me.on_report(report);
            }
        });
        match StatusServer::start(run_dir, sink).await {
            Ok(server) => {
                let _ = self.server.set(server);
            }
            Err(err) => {
                tracing::warn!(code = "status.server_failed", error = %err, "status server didn't start; using output heuristics only")
            }
        }
        let me = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                let Some(me) = me.upgrade() else { break };
                me.tick();
            }
        });
    }

    pub fn server(&self) -> Option<&StatusServer> {
        self.server.get()
    }

    /// An agent pane started: watch its output, and give it a fresh secret.
    /// Returns `(url, secret)` for its env when the server runs.
    pub fn watch(&self, pane_id: &str, waiting_patterns: &[String]) -> Option<(String, String)> {
        if let Ok(mut w) = self.watched.lock() {
            w.insert(
                pane_id.to_string(),
                Watched {
                    heuristics: Heuristics::new(waiting_patterns),
                    last_screen_check: None,
                },
            );
        }
        let server = self.server.get()?;
        Some((server.url(), server.register(pane_id)))
    }

    /// The pane closed or its agent left.
    pub fn unwatch(self: &Arc<Self>, pane_id: &str) {
        if let Ok(mut w) = self.watched.lock() {
            w.remove(pane_id);
        }
        if let Ok(mut a) = self.activity.lock() {
            a.remove(pane_id);
        }
        if let Some(server) = self.server.get() {
            server.unregister(pane_id);
        }
        let key = SessionKey::pane(pane_id);
        if self.store.remove(&key).is_some() {
            if let Ok(mut p) = self.pending.lock() {
                p.retain(|r| r.key != key);
            }
            if let Ok(mut r) = self.removed.lock() {
                r.push(key.0);
            }
            self.schedule();
        }
    }

    /// When the pane's engine last wrote, if it has (readiness, F-EN-06).
    pub fn last_output(&self, pane_id: &str) -> Option<i64> {
        self.activity.lock().ok()?.get(pane_id).copied()
    }

    pub fn is_watched(&self, pane_id: &str) -> bool {
        self.watched
            .lock()
            .map(|w| w.contains_key(pane_id))
            .unwrap_or(false)
    }

    /// The engine's session id from its hooks, if it reported one.
    pub fn engine_session(&self, pane_id: &str) -> Option<String> {
        self.sessions.lock().ok()?.get(pane_id).cloned()
    }

    /// Engine output reached the pane. `screen` reads its visible text
    /// (only called when a waiting-pattern check is due).
    pub fn output(self: &Arc<Self>, pane_id: &str, screen: impl FnOnce() -> Option<String>) {
        let now = self.clock.now_ms();
        if let Ok(mut a) = self.activity.lock() {
            a.insert(pane_id.to_string(), now);
        }
        let update = {
            let Ok(mut w) = self.watched.lock() else {
                return;
            };
            let Some(entry) = w.get_mut(pane_id) else {
                return;
            };
            let due = entry
                .last_screen_check
                .is_none_or(|t| now.saturating_sub(t) >= SCREEN_CHECK_MS);
            let text = if due {
                entry.last_screen_check = Some(now);
                screen()
            } else {
                None
            };
            entry.heuristics.output(now, text.as_deref())
        };
        if let Some(u) = update {
            self.apply(pane_id, u);
        }
    }

    /// Typing, resizing or a repaint: what follows is not the engine working.
    pub fn quiet_window(&self, pane_id: &str) {
        let now = self.clock.now_ms();
        if let Ok(mut w) = self.watched.lock() {
            if let Some(e) = w.get_mut(pane_id) {
                e.heuristics.quiet_window(now);
            }
        }
    }

    pub fn focus(self: &Arc<Self>, pane_id: &str, focused: bool) {
        let now = self.clock.now_ms();
        let update = self.watched.lock().ok().and_then(|mut w| {
            w.get_mut(pane_id)
                .and_then(|e| e.heuristics.focus(now, focused))
        });
        if let Some(u) = update {
            self.apply(pane_id, u);
        }
    }

    fn tick(self: &Arc<Self>) {
        let now = self.clock.now_ms();
        let updates: Vec<(String, StatusUpdate)> = match self.watched.lock() {
            Ok(mut w) => w
                .iter_mut()
                .filter_map(|(id, e)| e.heuristics.tick(now).map(|u| (id.clone(), u)))
                .collect(),
            Err(_) => return,
        };
        for (pane, u) in updates {
            self.apply(&pane, u);
        }
    }

    /// A verified hook report.
    pub fn on_report(self: &Arc<Self>, report: StatusReport) {
        let pane = report.pane_id.clone();
        if let Ok(mut w) = self.watched.lock() {
            match w.get_mut(&pane) {
                Some(e) => e.heuristics.hooked(),
                // A report for a pane that isn't running an agent any more.
                None => return,
            }
        }
        if let Some(id) = &report.payload.session_id {
            let changed = self
                .sessions
                .lock()
                .map(|mut s| s.insert(pane.clone(), id.clone()).as_ref() != Some(id))
                .unwrap_or(false);
            if changed {
                if let Some(sink) = self.on_session.get() {
                    sink(&pane, id);
                }
            }
        }
        match report.event {
            ReportEvent::Title => {
                let key = SessionKey::pane(&pane);
                if let (Some(title), Some(cur)) =
                    (report.payload.title.clone(), self.store.get(&key))
                {
                    let update = StatusUpdate {
                        status: cur.status,
                        reason: cur.reason,
                        ts: report.ts.max(cur.ts),
                        tool: cur.tool,
                        title: Some(title),
                        preview: cur.preview,
                    };
                    self.apply(&pane, update);
                }
            }
            ReportEvent::Session => {}
            _ => {
                if let Some(u) = report.update() {
                    self.apply(&pane, u);
                }
            }
        }
    }

    fn apply(self: &Arc<Self>, pane_id: &str, update: StatusUpdate) {
        if let Some(rec) = self.store.apply(&SessionKey::pane(pane_id), update) {
            self.queue(rec);
        }
    }

    /// Queues a changed record; everything queued within [`COALESCE`] goes
    /// out as one `status:changed`.
    fn queue(self: &Arc<Self>, rec: StatusRecord) {
        if let Ok(mut p) = self.pending.lock() {
            p.retain(|r| r.key != rec.key);
            p.push(rec);
        }
        self.schedule();
    }

    fn schedule(self: &Arc<Self>) {
        {
            let Ok(mut scheduled) = self.flushing.lock() else {
                return;
            };
            if *scheduled {
                return;
            }
            *scheduled = true;
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(COALESCE).await;
            if let Ok(mut f) = me.flushing.lock() {
                *f = false;
            }
            let records = me
                .pending
                .lock()
                .map(|mut p| std::mem::take(&mut *p))
                .unwrap_or_default();
            let removed = me
                .removed
                .lock()
                .map(|mut r| std::mem::take(&mut *r))
                .unwrap_or_default();
            if !records.is_empty() || !removed.is_empty() {
                me.bus.publish(BusEvent::new(
                    Topic::Status,
                    "status:changed",
                    serde_json::json!({ "records": records, "removed": removed }),
                ));
            }
        });
    }

    pub fn snapshot(&self) -> Vec<StatusRecord> {
        self.store.snapshot()
    }

    /// Sets the status of a non-pane source and remembers its card details.
    pub fn set_external(self: &Arc<Self>, meta: ExternalMeta, mut update: StatusUpdate) {
        let Some(key) = meta.source.session_key() else {
            return;
        };
        update.ts = update.ts.max(self.clock.now_ms());
        if let Ok(mut e) = self.external.lock() {
            e.insert(key.0.clone(), meta);
        }
        if let Some(rec) = self.store.apply(&key, update) {
            self.queue(rec);
        }
    }

    /// Marks a done source as seen: it becomes idle.
    pub fn set_idle(self: &Arc<Self>, key: &SessionKey) {
        let Some(cur) = self.store.get(key) else {
            return;
        };
        let update = StatusUpdate {
            status: tandem_core::status::AgentStatus::Idle,
            reason: None,
            ts: cur.ts.max(self.clock.now_ms()),
            tool: cur.tool,
            title: cur.title,
            preview: cur.preview,
        };
        if let Some(rec) = self.store.apply(key, update) {
            self.queue(rec);
        }
    }

    pub fn external(&self, key: &SessionKey) -> Option<ExternalMeta> {
        self.external.lock().ok()?.get(&key.0).cloned()
    }

    pub fn remove_external(self: &Arc<Self>, key: &SessionKey) {
        if let Ok(mut e) = self.external.lock() {
            e.remove(&key.0);
        }
        if self.store.remove(key).is_some() {
            if let Ok(mut p) = self.pending.lock() {
                p.retain(|r| &r.key != key);
            }
            if let Ok(mut r) = self.removed.lock() {
                r.push(key.0.clone());
            }
            self.schedule();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tandem_core::clock::MockClock;
    use tandem_core::status::AgentStatus;
    use tandem_core::status_report::{ReportEvent, ReportPayload};

    fn service() -> (Arc<StatusService>, MockClock) {
        let start = chrono::DateTime::parse_from_rfc3339("2026-10-01T09:00:00Z")
            .expect("time")
            .with_timezone(&chrono::Utc);
        let clock = MockClock::new(start);
        (
            StatusService::new(EventBus::new(), Arc::new(clock.clone())),
            clock,
        )
    }

    fn status(s: &Arc<StatusService>, pane: &str) -> Option<AgentStatus> {
        s.store.get(&SessionKey::pane(pane)).map(|r| r.status)
    }

    #[test]
    fn f_en_05_3_output_drives_the_heuristics_from_the_first_byte() {
        let (s, clock) = service();
        s.watch("pn_1", &[]);
        // The first output (no earlier screen check) must not trip anything.
        s.output("pn_1", || Some("hello".into()));
        assert_eq!(status(&s, "pn_1"), Some(AgentStatus::Working));
        clock.advance(Duration::from_secs(31));
        s.tick();
        assert_eq!(status(&s, "pn_1"), Some(AgentStatus::Done));
        assert!(s.last_output("pn_1").is_some());
        // Output for a pane nobody watches is ignored.
        s.output("pn_x", || None);
        assert_eq!(status(&s, "pn_x"), None);
    }

    #[test]
    fn f_sh_07_2_a_reported_session_id_is_persisted_once_per_change() {
        let (s, _clock) = service();
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let sink = seen.clone();
        s.set_session_sink(Arc::new(move |pane, id| {
            if let Ok(mut v) = sink.lock() {
                v.push((pane.to_string(), id.to_string()));
            }
        }));
        s.watch("pn_1", &[]);
        let report = |sid: &str, ts| StatusReport {
            v: 1,
            pane_id: "pn_1".into(),
            event: ReportEvent::Session,
            ts,
            payload: ReportPayload {
                session_id: Some(sid.into()),
                ..ReportPayload::default()
            },
        };
        s.on_report(report("a-1", 1));
        s.on_report(report("a-1", 2));
        s.on_report(report("b-2", 3));
        assert_eq!(
            *seen.lock().expect("seen"),
            vec![("pn_1".into(), "a-1".into()), ("pn_1".into(), "b-2".into())]
        );
    }

    #[test]
    fn f_en_05_2_hook_reports_win_and_stale_ones_are_dropped() {
        let (s, _clock) = service();
        s.watch("pn_1", &[]);
        let report = |event, ts| StatusReport {
            v: 1,
            pane_id: "pn_1".into(),
            event,
            ts,
            payload: ReportPayload {
                session_id: Some("ses-9".into()),
                ..ReportPayload::default()
            },
        };
        s.on_report(report(ReportEvent::Working, 100));
        s.on_report(report(ReportEvent::Waiting, 200));
        assert_eq!(status(&s, "pn_1"), Some(AgentStatus::Waiting));
        s.on_report(report(ReportEvent::Working, 150));
        assert_eq!(
            status(&s, "pn_1"),
            Some(AgentStatus::Waiting),
            "stale report"
        );
        s.on_report(report(ReportEvent::Done, 300));
        assert_eq!(status(&s, "pn_1"), Some(AgentStatus::Done));
        assert_eq!(s.engine_session("pn_1").as_deref(), Some("ses-9"));
        // Hooks reported: output no longer flips the pane to working.
        s.output("pn_1", || None);
        assert_eq!(status(&s, "pn_1"), Some(AgentStatus::Done));
        s.unwatch("pn_1");
        assert_eq!(status(&s, "pn_1"), None);
    }
}
