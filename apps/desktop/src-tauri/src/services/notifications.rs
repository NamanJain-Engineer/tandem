//! The notification center and OS notifications (F-DB-02, F-DB-03).
//!
//! Items come from the status bus (an agent waiting, done or failed) and the
//! approval broker; they resolve when their source moves on. An OS
//! notification goes out only when Tandem isn't focused or the agent isn't
//! on screen (unless the user wants them always), per type.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast::error::RecvError;

use tandem_core::approvals::ApprovalEvent;
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::settings::SettingKey;
use tandem_core::source::SourceRef;
use tandem_core::status::{AgentStatus, SessionKey, StatusRecord, WaitingReason};
use tandem_store::repo::notifications::{
    self as repo, NewNotification, Notification, NotificationKind,
};
use tandem_store::repo::settings;
use tandem_store::Store;

use crate::services::dashboard::{self, DashboardItem};
use crate::services::notifier::{Notifier, OsNotification, RecordingNotifier};
use crate::state::AppState;

/// Per-type channels (UI-41 defaults): `(system, sound)`.
fn default_channels(kind: NotificationKind) -> (bool, bool) {
    match kind {
        NotificationKind::Approval | NotificationKind::Question => (true, true),
        NotificationKind::Done | NotificationKind::Failed | NotificationKind::RoutineReady => {
            (true, false)
        }
        _ => (false, false),
    }
}

/// The settings key a type's toggles live under in `notify_types`.
fn type_key(kind: NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Approval | NotificationKind::Question => "approval",
        NotificationKind::Done => "done",
        NotificationKind::Failed => "failed",
        NotificationKind::RoutineReady => "routines",
        NotificationKind::TaskReview => "tasks",
        _ => "other",
    }
}

pub struct NotificationService {
    store: Store,
    bus: EventBus,
    notifier: OnceLock<Arc<dyn Notifier>>,
    /// The test harness's notifier, to read and click its calls.
    pub recording: OnceLock<Arc<RecordingNotifier>>,
    focused: AtomicBool,
    /// Test harness: pretend the window is (un)focused.
    focus_override: Mutex<Option<bool>>,
    visible: Mutex<HashSet<String>>,
    /// Each session's last status and when it began (`since`).
    last: Mutex<HashMap<String, (AgentStatus, i64)>>,
}

impl NotificationService {
    pub fn new(store: Store, bus: EventBus) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            notifier: OnceLock::new(),
            recording: OnceLock::new(),
            focused: AtomicBool::new(true),
            focus_override: Mutex::new(None),
            visible: Mutex::default(),
            last: Mutex::default(),
        })
    }

    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Relaxed);
    }

    pub fn override_focus(&self, focused: Option<bool>) {
        if let Ok(mut f) = self.focus_override.lock() {
            *f = focused;
        }
    }

    fn is_focused(&self) -> bool {
        self.focus_override
            .lock()
            .ok()
            .and_then(|f| *f)
            .unwrap_or_else(|| self.focused.load(Ordering::Relaxed))
    }

    /// What the webview shows (`ui_visibility`).
    pub fn set_visible(&self, keys: Vec<String>) {
        if let Ok(mut v) = self.visible.lock() {
            *v = keys.into_iter().collect();
        }
    }

    /// The system notifier, or (test harness) a recording one.
    pub fn start(
        self: &Arc<Self>,
        app: AppHandle,
        notifier: Arc<dyn Notifier>,
        recording: Option<Arc<RecordingNotifier>>,
    ) {
        let _ = self.notifier.set(notifier);
        if let Some(r) = recording {
            let _ = self.recording.set(r);
        }
        let me = self.clone();
        let handle = app.clone();
        let mut status = self.bus.subscribe(Topic::Status);
        tauri::async_runtime::spawn(async move {
            if let Err(err) = repo::prune(&me.store).await {
                tracing::warn!(error = %err, "couldn't prune notifications");
            }
            loop {
                match status.recv().await {
                    Ok(event) => me.on_status(&handle, event).await,
                    Err(RecvError::Lagged(n)) => tracing::warn!(missed = n, "status events lagged"),
                    Err(RecvError::Closed) => break,
                }
            }
        });
        let me = self.clone();
        let handle = app;
        let mut approvals = handle.state::<AppState>().approvals.broker.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                match approvals.recv().await {
                    Ok(event) => me.on_approval(&handle, event).await,
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "approval events lagged")
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    async fn on_status(&self, app: &AppHandle, event: BusEvent) {
        if event.name != "status:changed" {
            return;
        }
        let records: Vec<StatusRecord> = event
            .payload
            .get("records")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let removed: Vec<String> = event
            .payload
            .get("removed")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        for key in removed {
            if let Ok(mut l) = self.last.lock() {
                l.remove(&key);
            }
            self.resolve(&format!("waiting:{key}")).await;
        }
        for rec in records {
            let prev = self
                .last
                .lock()
                .ok()
                .and_then(|mut l| l.insert(rec.key.0.clone(), (rec.status, rec.since)));
            if prev == Some((rec.status, rec.since)) {
                continue;
            }
            if prev.map(|p| p.0) == Some(AgentStatus::Waiting) && rec.status != AgentStatus::Waiting
            {
                self.resolve(&format!("waiting:{}", rec.key.0)).await;
            }
            let Some(kind) = notification_for(prev, &rec) else {
                continue;
            };
            let state = app.state::<AppState>();
            let Some(item) = dashboard::item_for(&state, rec.clone()).await else {
                continue;
            };
            let name = display_name(&item);
            let title = match kind {
                NotificationKind::Question => format!("{name} has a question"),
                NotificationKind::Approval => format!("{name} needs approval"),
                NotificationKind::Done => format!("{name} finished"),
                _ => format!("{name} stopped with an error"),
            };
            let dedupe =
                (rec.status == AgentStatus::Waiting).then(|| format!("waiting:{}", rec.key.0));
            self.raise(
                &state,
                kind,
                title,
                source_line(&item, None),
                item.source.clone(),
                &rec.key,
                dedupe,
            )
            .await;
        }
    }

    async fn on_approval(&self, app: &AppHandle, event: ApprovalEvent) {
        match event {
            ApprovalEvent::Requested(view) => {
                let state = app.state::<AppState>();
                let key = view.source.session_key();
                let item = match key
                    .as_ref()
                    .and_then(|k| state.terminals.status.store.get(k))
                {
                    Some(rec) => dashboard::item_for(&state, rec).await,
                    None => None,
                };
                let name = item
                    .as_ref()
                    .map(display_name)
                    .unwrap_or_else(|| "An agent".into());
                let body = match &item {
                    Some(i) => source_line(i, Some(&view.summary)),
                    None => view.summary.clone(),
                };
                let key =
                    key.unwrap_or_else(|| SessionKey(format!("approval:{}", view.id.as_str())));
                self.raise(
                    &state,
                    NotificationKind::Approval,
                    format!("{name} needs approval"),
                    body,
                    view.source.clone(),
                    &key,
                    Some(format!("approval:{}", view.id.as_str())),
                )
                .await;
            }
            ApprovalEvent::Resolved { view, .. } => {
                self.resolve(&format!("approval:{}", view.id.as_str()))
                    .await;
            }
        }
    }

    /// An in-app notification that isn't about an agent session (worktree
    /// clean-up): it replaces the unresolved one with the same `dedupe_key`
    /// and never reaches the OS.
    pub async fn raise_in_app(
        &self,
        kind: NotificationKind,
        title: String,
        body: String,
        source: SourceRef,
        dedupe_key: String,
    ) {
        self.resolve(&dedupe_key).await;
        match repo::insert(
            &self.store,
            NewNotification {
                kind,
                title,
                body,
                source,
                dedupe_key: Some(dedupe_key),
            },
        )
        .await
        {
            Ok(n) => self.bus.publish(BusEvent::new(
                Topic::Notifications,
                "notifications:new",
                serde_json::json!({ "notification": n, "sound": false }),
            )),
            Err(err) => tracing::warn!(error = %err, "couldn't store a notification"),
        }
    }

    /// A pane's notification through the usual channels (Auto ending, F-AU-01).
    pub async fn raise_for_pane(
        &self,
        state: &AppState,
        kind: NotificationKind,
        title: String,
        body: String,
        pane_id: &str,
    ) {
        self.raise(
            state,
            kind,
            title,
            body,
            SourceRef::Pane {
                pane_id: pane_id.to_string(),
            },
            &SessionKey::pane(pane_id),
            None,
        )
        .await;
    }

    /// Resolves an in-app item by its dedupe key (the credit-low item).
    pub async fn resolve_key(&self, dedupe: &str) {
        self.resolve(dedupe).await;
    }

    async fn resolve(&self, dedupe: &str) {
        match repo::resolve(&self.store, dedupe).await {
            Ok(ids) if !ids.is_empty() => self.bus.publish(BusEvent::new(
                Topic::Notifications,
                "notifications:changed",
                serde_json::json!({ "ids": ids, "state": "resolved" }),
            )),
            Ok(_) => {}
            Err(err) => tracing::warn!(error = %err, "couldn't resolve notifications"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn raise(
        &self,
        state: &AppState,
        kind: NotificationKind,
        title: String,
        body: String,
        source: SourceRef,
        key: &SessionKey,
        dedupe_key: Option<String>,
    ) {
        let n = match repo::insert(
            &self.store,
            NewNotification {
                kind,
                title,
                body,
                source,
                dedupe_key,
            },
        )
        .await
        {
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(error = %err, "couldn't store a notification");
                return;
            }
        };
        let os = self.should_notify(state, &n, key).await;
        if os.system {
            if let Some(notifier) = self.notifier.get() {
                notifier.notify(&OsNotification {
                    kind: n.kind,
                    title: n.title.clone(),
                    body: n.body.clone(),
                    source: n.source.clone(),
                });
                if os.attention {
                    notifier.attention();
                }
            }
        }
        self.bus.publish(BusEvent::new(
            Topic::Notifications,
            "notifications:new",
            serde_json::json!({ "notification": n, "sound": os.sound }),
        ));
    }

    async fn should_notify(
        &self,
        state: &AppState,
        n: &Notification,
        key: &SessionKey,
    ) -> Channels {
        let prefs = Prefs::load(state).await;
        let focused = self.is_focused();
        let seen = focused
            && self
                .visible
                .lock()
                .map(|v| v.contains(&key.0))
                .unwrap_or(false);
        decide(n.kind, &prefs, focused, seen)
    }

    /// Clicking a (recorded) OS notification: focus the app and the source.
    pub async fn click(app: &AppHandle, source: SourceRef) {
        let state = app.state::<AppState>();
        let _ = crate::commands::shell::focus(app, &state, source).await;
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Channels {
    system: bool,
    sound: bool,
    attention: bool,
}

/// The notification settings (UI-41).
struct Prefs {
    system: bool,
    only_unseen: bool,
    sound: bool,
    attention: bool,
    types: Value,
}

impl Prefs {
    async fn load(state: &AppState) -> Self {
        let get = |k| settings::get(&state.store, k);
        let on = |v: Result<Value, _>, default: bool| {
            v.ok().and_then(|v| v.as_bool()).unwrap_or(default)
        };
        Self {
            system: on(get(SettingKey::NotifySystem).await, true),
            only_unseen: on(get(SettingKey::NotifyOnlyUnseen).await, true),
            sound: on(get(SettingKey::NotifySound).await, true),
            attention: on(get(SettingKey::NotifyAttention).await, true),
            types: get(SettingKey::NotifyTypes).await.unwrap_or(Value::Null),
        }
    }
}

/// F-DB-03: a system notification when the type's toggle is on and the
/// user can't see the source (or wants them always); sound and attention
/// ride on it.
fn decide(kind: NotificationKind, p: &Prefs, focused: bool, seen: bool) -> Channels {
    let (def_system, def_sound) = default_channels(kind);
    // `notify_types` keys are "<type>.<channel>" (settings maps are flat).
    let channel = |c: &str| {
        p.types
            .get(format!("{}.{c}", type_key(kind)))
            .and_then(Value::as_bool)
    };
    let type_system = channel("system").unwrap_or(def_system);
    let type_sound = channel("sound").unwrap_or(def_sound);
    let system = p.system && type_system && !(p.only_unseen && seen);
    Channels {
        system,
        sound: system && p.sound && type_sound,
        attention: system && p.attention && !focused,
    }
}

/// The agent's first name, else its work title, else the engine.
fn display_name(item: &DashboardItem) -> String {
    item.human_name
        .clone()
        .or_else(|| {
            item.title
                .clone()
                .map(|t| strip_spinner(&t))
                .filter(|t| !t.is_empty())
        })
        .or_else(|| item.engine_label.clone())
        .unwrap_or_else(|| "An agent".into())
}

fn strip_spinner(t: &str) -> String {
    t.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim()
        .to_string()
}

/// "Claude Code · shop — Run `pnpm test`" (UI-40), redacted upstream.
fn source_line(item: &DashboardItem, detail: Option<&str>) -> String {
    let head = [item.engine_label.as_deref(), item.workspace.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    match detail {
        Some(d) if !head.is_empty() => format!("{head} — {d}"),
        Some(d) => d.to_string(),
        None => head,
    }
}

/// What a status change notifies, given the session's previous status and
/// when it began. A "done" that began after the last one seen is a new
/// finished turn even when its "working" fell inside one status batch (a
/// quick turn), so it notifies too.
fn notification_for(
    prev: Option<(AgentStatus, i64)>,
    rec: &StatusRecord,
) -> Option<NotificationKind> {
    let again = prev.is_some_and(|(_, since)| since != rec.since);
    match (rec.status, prev.map(|p| p.0)) {
        (_, Some(status)) if status == rec.status && !again => None,
        // Terminal agents wait on their own dialogs; approvals from the
        // broker notify through `on_approval`.
        (AgentStatus::Waiting, _) if rec.key.0.starts_with("pane:") => Some(match rec.reason {
            Some(WaitingReason::Question) => NotificationKind::Question,
            _ => NotificationKind::Approval,
        }),
        (AgentStatus::Done, Some(AgentStatus::Working | AgentStatus::Waiting)) => {
            Some(NotificationKind::Done)
        }
        (AgentStatus::Done, Some(AgentStatus::Done | AgentStatus::Idle)) if again => {
            Some(NotificationKind::Done)
        }
        (AgentStatus::Failed, _) => Some(NotificationKind::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(types: Value) -> Prefs {
        Prefs {
            system: true,
            only_unseen: true,
            sound: true,
            attention: true,
            types,
        }
    }

    fn rec(status: AgentStatus, since: i64) -> StatusRecord {
        serde_json::from_value(serde_json::json!({
            "key": "pane:pn_1", "status": status, "reason": null, "since": since, "ts": since,
            "tool": null, "title": null, "preview": null
        }))
        .expect("record")
    }

    #[test]
    fn f_db_03_a_quick_turn_whose_working_state_was_batched_away_still_notifies() {
        use AgentStatus::*;
        // working → done seen separately.
        assert_eq!(
            notification_for(Some((Working, 1)), &rec(Done, 2)),
            Some(NotificationKind::Done)
        );
        // The same done again (a re-sent batch): nothing.
        assert_eq!(notification_for(Some((Done, 2)), &rec(Done, 2)), None);
        // A second turn whose working fell inside one batch: done began anew.
        assert_eq!(
            notification_for(Some((Done, 2)), &rec(Done, 9)),
            Some(NotificationKind::Done)
        );
        assert_eq!(
            notification_for(Some((Idle, 2)), &rec(Done, 9)),
            Some(NotificationKind::Done)
        );
        // At startup a restored done isn't news.
        assert_eq!(notification_for(None, &rec(Done, 2)), None);
        assert_eq!(notification_for(Some((Done, 2)), &rec(Idle, 5)), None);
        assert_eq!(
            notification_for(Some((Working, 2)), &rec(Failed, 5)),
            Some(NotificationKind::Failed)
        );
    }

    #[test]
    fn f_db_03_1_unfocused_done_notifies_once_with_attention_and_no_sound() {
        let c = decide(NotificationKind::Done, &prefs(Value::Null), false, false);
        assert_eq!(
            c,
            Channels {
                system: true,
                sound: false,
                attention: true
            }
        );
        // An approval also plays the sound by default.
        assert!(
            decide(
                NotificationKind::Approval,
                &prefs(Value::Null),
                false,
                false
            )
            .sound
        );
    }

    #[test]
    fn f_db_03_2_the_done_toggle_off_sends_nothing() {
        let p = prefs(serde_json::json!({ "done.system": false }));
        assert!(!decide(NotificationKind::Done, &p, false, false).system);
        assert!(decide(NotificationKind::Failed, &p, false, false).system);
    }

    #[test]
    fn f_db_03_a_source_on_screen_in_a_focused_window_is_not_notified() {
        let p = prefs(Value::Null);
        assert!(!decide(NotificationKind::Done, &p, true, true).system);
        // Focused but somewhere else: notify, no attention request.
        assert_eq!(
            decide(NotificationKind::Done, &p, true, false),
            Channels {
                system: true,
                sound: false,
                attention: false
            }
        );
        // "Only when I can't see the agent" off: always.
        let always = Prefs {
            only_unseen: false,
            ..prefs(Value::Null)
        };
        assert!(decide(NotificationKind::Done, &always, true, true).system);
    }
}
