//! Auto (F-AU-01, ARCHITECTURE §8.8): a supervisor that keeps an agent pane
//! moving toward a goal. Each turn waits until the agent is idle at its
//! prompt (never while it waits for an approval), sends the last 200 lines of
//! the pane (redacted, at most 16 KB) to `/v1/auto/next`, and types the
//! answer. The turn ceiling is held here whatever the server says, and
//! nothing is typed once the agent has exited, the pane closed or the engine
//! changed. Runs live in memory; the pane is the thing that persists.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{AutoRunId, PaneId};
use tandem_core::status::{AgentStatus, StatusRecord, WaitingReason};
use tandem_core::{ErrorCode, IpcError};
use tokio::sync::Notify;
use ts_rs::TS;

use crate::services::api::{ApiError, AutoNextRequest, AutoNextResponse};

/// Lines of the pane sent per turn.
pub const SCROLLBACK_LINES: usize = 200;
/// The most scrollback sent per turn (bytes, the end of the text).
pub const SCROLLBACK_BYTES: usize = 16 * 1024;
/// The highest ceiling a run can have (the API refuses more).
pub const MAX_CEILING: u32 = 30;
/// A goal's longest length (characters).
pub const MAX_GOAL: usize = 2000;
/// Typing that fails this many turns in a row ends the run as stuck.
const TYPE_FAILURES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutoMode {
    /// Keep prompting until the task is complete and verified.
    Finish,
    /// Stress-test: tests, edge cases and error handling.
    Harden,
    /// Self-review the diff and fix issues.
    Review,
    /// Propose and try small improvements.
    Explore,
    /// The user's own goal.
    Custom,
}

impl AutoMode {
    /// The turn ceiling per mode (PRODUCT_SPEC F-AU-01).
    pub fn default_ceiling(self) -> u32 {
        match self {
            AutoMode::Finish => 20,
            AutoMode::Harden => 15,
            AutoMode::Review => 8,
            AutoMode::Explore => 10,
            AutoMode::Custom => 10,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AutoMode::Finish => "finish",
            AutoMode::Harden => "harden",
            AutoMode::Review => "review",
            AutoMode::Explore => "explore",
            AutoMode::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AutoMode::Finish => "Finish",
            AutoMode::Harden => "Stress-test",
            AutoMode::Review => "Review",
            AutoMode::Explore => "Explore",
            AutoMode::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutoState {
    Running,
    Paused,
    Ended,
}

/// Why a run is paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutoPause {
    /// The user paused it.
    User,
    /// The agent waits for an approval; Auto resumes once it's answered.
    ApprovalPending,
}

/// Every way a run ends (PRODUCT_SPEC F-AU-01 "End reasons", plus `turned_off`, D-619).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutoEndReason {
    Done,
    Stuck,
    TurnCeiling,
    OutOfCredits,
    NoSubscription,
    /// A Team admin turned Auto off.
    TurnedOff,
    SignedOut,
    ApiUnreachable,
    PaneClosed,
    EngineExited,
    EngineChanged,
    UserStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutoRunView {
    pub run_id: AutoRunId,
    pub pane_id: PaneId,
    pub mode: AutoMode,
    pub goal: Option<String>,
    pub ceiling: u32,
    /// Prompts typed so far.
    pub turns: u32,
    pub state: AutoState,
    pub paused: Option<AutoPause>,
    pub end_reason: Option<AutoEndReason>,
    /// The supervisor's reason for done or stuck, or the server's message.
    pub detail: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

/// What Auto knows about its pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneCheck {
    Closed,
    /// A plain shell pane.
    Shell,
    /// An agent pane: its engine, and whether the engine process still runs
    /// (false once it has exited or handed the pane back to the shell).
    Agent {
        engine_id: Option<String>,
        alive: bool,
    },
}

/// The app around a run; faked in tests.
#[async_trait]
pub trait AutoHost: Send + Sync {
    async fn pane(&self, pane: &str) -> PaneCheck;
    fn status(&self, pane: &str) -> Option<StatusRecord>;
    /// The pane's recent plain text (ANSI already stripped), not yet redacted.
    fn tail(&self, pane: &str) -> String;
    async fn type_prompt(&self, pane: &str, text: &str) -> Result<(), String>;
    async fn next(&self, req: &AutoNextRequest) -> Result<AutoNextResponse, ApiError>;
    fn now_ms(&self) -> i64;
    /// The run ended (the notification).
    async fn ended(&self, view: &AutoRunView);
}

#[derive(Debug, Clone)]
pub struct AutoTiming {
    /// How often a waiting run looks again without a status event.
    pub poll: Duration,
    /// Waits before the 3 retries of an unreachable API.
    pub backoff: [Duration; 3],
}

impl Default for AutoTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            backoff: [
                Duration::from_secs(1),
                Duration::from_secs(3),
                Duration::from_secs(9),
            ],
        }
    }
}

#[derive(Default)]
struct Control {
    wake: Notify,
    stopped: AtomicBool,
}

struct Run {
    view: AutoRunView,
    control: Arc<Control>,
}

/// The scrollback one turn sends: the last 200 lines, redacted, then at most
/// the last 16 KB (on a character boundary).
pub fn scrollback(raw: &str) -> String {
    let text =
        tandem_core::redact::redact(&tandem_engines::handoff::last_lines(raw, SCROLLBACK_LINES));
    if text.len() <= SCROLLBACK_BYTES {
        return text;
    }
    let mut cut = text.len() - SCROLLBACK_BYTES;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    text[cut..].to_string()
}

enum Readiness {
    Ready,
    Approval,
    NotYet,
}

/// The agent is at its prompt, in a status entered after the last prompt was
/// typed (`typed_on` is the status it had then), so a turn never reads the
/// state from before the agent took the prompt. Status times come from the
/// app clock, which tests can stop, so this compares records, not times.
fn readiness(record: Option<&StatusRecord>, typed_on: Option<(AgentStatus, i64)>) -> Readiness {
    let Some(r) = record else {
        return Readiness::NotYet;
    };
    match (r.status, r.reason) {
        (AgentStatus::Waiting, Some(WaitingReason::Approval | WaitingReason::Question)) => {
            Readiness::Approval
        }
        (AgentStatus::Working, _) => Readiness::NotYet,
        _ if typed_on != Some((r.status, r.since)) => Readiness::Ready,
        _ => Readiness::NotYet,
    }
}

enum Failure {
    Retry,
    End(AutoEndReason, Option<String>),
}

fn classify(err: &ApiError) -> Failure {
    match err {
        ApiError::Network(_) => Failure::Retry,
        ApiError::Http {
            status,
            code,
            detail,
        } => match (*status, code.as_str()) {
            (401, _) => Failure::End(AutoEndReason::SignedOut, None),
            (402, _) => Failure::End(AutoEndReason::OutOfCredits, detail.clone()),
            (403, "FORBIDDEN_POLICY") => Failure::End(AutoEndReason::TurnedOff, detail.clone()),
            (403, _) => Failure::End(AutoEndReason::NoSubscription, detail.clone()),
            (429, _) => Failure::Retry,
            _ => Failure::End(AutoEndReason::ApiUnreachable, detail.clone()),
        },
        ApiError::Decode(_) => Failure::End(
            AutoEndReason::ApiUnreachable,
            Some("Vibezeo answered in a way this version doesn't understand.".into()),
        ),
    }
}

/// Why the pane can't take a prompt any more, if it can't.
fn gone(check: &PaneCheck, engine_id: &Option<String>) -> Option<AutoEndReason> {
    match check {
        PaneCheck::Closed => Some(AutoEndReason::PaneClosed),
        PaneCheck::Shell => Some(AutoEndReason::EngineExited),
        PaneCheck::Agent { engine_id: e, .. } if e != engine_id => {
            Some(AutoEndReason::EngineChanged)
        }
        PaneCheck::Agent { alive: false, .. } => Some(AutoEndReason::EngineExited),
        PaneCheck::Agent { .. } => None,
    }
}

pub struct AutoService {
    bus: EventBus,
    timing: AutoTiming,
    runs: Mutex<HashMap<AutoRunId, Run>>,
    listening: AtomicBool,
}

impl AutoService {
    pub fn new(bus: EventBus, timing: AutoTiming) -> Self {
        Self {
            bus,
            timing,
            runs: Mutex::new(HashMap::new()),
            listening: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<AutoRunId, Run>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every run the UI should show (live ones and ended ones not yet dismissed).
    pub fn list(&self) -> Vec<AutoRunView> {
        let mut runs: Vec<AutoRunView> = self.lock().values().map(|r| r.view.clone()).collect();
        runs.sort_by_key(|r| r.started_at);
        runs
    }

    pub fn get(&self, run: &AutoRunId) -> Option<AutoRunView> {
        self.lock().get(run).map(|r| r.view.clone())
    }

    fn snapshot(&self, run: &AutoRunId) -> Option<(AutoRunView, Arc<Control>)> {
        self.lock()
            .get(run)
            .map(|r| (r.view.clone(), r.control.clone()))
    }

    fn emit(&self, view: &AutoRunView) {
        self.bus
            .publish(BusEvent::new(Topic::Status, "auto:changed", view));
    }

    fn update(&self, run: &AutoRunId, f: impl FnOnce(&mut AutoRunView)) -> Option<AutoRunView> {
        let view = {
            let mut runs = self.lock();
            let entry = runs.get_mut(run)?;
            f(&mut entry.view);
            entry.control.wake.notify_one();
            entry.view.clone()
        };
        self.emit(&view);
        Some(view)
    }

    /// Starts a run on an agent pane. The caller has checked the plan, the
    /// policy, the consent and that the pane is an agent pane.
    pub fn start(
        self: &Arc<Self>,
        host: Arc<dyn AutoHost>,
        pane: PaneId,
        engine_id: Option<String>,
        mode: AutoMode,
        goal: Option<String>,
        ceiling: Option<u32>,
    ) -> Result<AutoRunView, IpcError> {
        let goal = goal.map(|g| g.trim().to_string()).filter(|g| !g.is_empty());
        if mode == AutoMode::Custom && goal.is_none() {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Write the goal Auto should work toward.",
            ));
        }
        if goal.as_ref().is_some_and(|g| g.chars().count() > MAX_GOAL) {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Keep the goal under 2,000 characters.",
            ));
        }
        let ceiling = match (mode, ceiling) {
            (AutoMode::Custom, Some(c)) if (1..=MAX_CEILING).contains(&c) => c,
            (AutoMode::Custom, Some(_)) => {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "Pick between 1 and 30 turns.",
                ))
            }
            _ => mode.default_ceiling(),
        };
        let view = {
            let mut runs = self.lock();
            if runs
                .values()
                .any(|r| r.view.pane_id == pane && r.view.state != AutoState::Ended)
            {
                return Err(IpcError::new(
                    ErrorCode::Conflict,
                    "Auto is already running in this pane.",
                ));
            }
            runs.retain(|_, r| r.view.pane_id != pane);
            let view = AutoRunView {
                run_id: AutoRunId::new(),
                pane_id: pane,
                mode,
                goal,
                ceiling,
                turns: 0,
                state: AutoState::Running,
                paused: None,
                end_reason: None,
                detail: None,
                started_at: host.now_ms(),
                ended_at: None,
            };
            runs.insert(
                view.run_id.clone(),
                Run {
                    view: view.clone(),
                    control: Arc::default(),
                },
            );
            view
        };
        self.emit(&view);
        let this = self.clone();
        let run = view.run_id.clone();
        tokio::spawn(async move { this.drive(host, run, engine_id).await });
        Ok(view)
    }

    fn not_found() -> IpcError {
        IpcError::new(ErrorCode::NotFound, "That Auto run has ended.")
    }

    pub fn pause(&self, run: &AutoRunId) -> Result<AutoRunView, IpcError> {
        self.update(run, |v| {
            if v.state != AutoState::Ended {
                v.state = AutoState::Paused;
                v.paused = Some(AutoPause::User);
            }
        })
        .ok_or_else(Self::not_found)
    }

    pub fn resume(&self, run: &AutoRunId) -> Result<AutoRunView, IpcError> {
        self.update(run, |v| {
            if v.paused == Some(AutoPause::User) {
                v.state = AutoState::Running;
                v.paused = None;
            }
        })
        .ok_or_else(Self::not_found)
    }

    /// Stops at once: nothing more is typed, even if an answer is on its way.
    pub async fn stop(
        &self,
        host: &dyn AutoHost,
        run: &AutoRunId,
    ) -> Result<AutoRunView, IpcError> {
        if let Some((view, control)) = self.snapshot(run) {
            control.stopped.store(true, Ordering::SeqCst);
            if view.state == AutoState::Ended {
                return Ok(view);
            }
        }
        self.end(host, run, AutoEndReason::UserStopped, None)
            .await
            .ok_or_else(Self::not_found)
    }

    /// Clears an ended run's chip.
    pub fn dismiss(&self, run: &AutoRunId) {
        let removed = {
            let mut runs = self.lock();
            match runs.get(run) {
                Some(r) if r.view.state == AutoState::Ended => runs.remove(run).is_some(),
                _ => false,
            }
        };
        if removed {
            self.bus.publish(BusEvent::new(
                Topic::Status,
                "auto:dismissed",
                serde_json::json!({ "runId": run }),
            ));
        }
    }

    /// Wakes every run (a status change; they look at their own pane).
    pub fn poke(&self) {
        for r in self.lock().values() {
            r.control.wake.notify_one();
        }
    }

    /// Follows status changes so a run reacts at once, not at its next poll.
    pub fn listen(self: &Arc<Self>) {
        if self.listening.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut rx = self.bus.subscribe(Topic::Status);
        let this = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) if ev.name == "status:changed" => match this.upgrade() {
                        Some(s) => s.poke(),
                        None => return,
                    },
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    async fn end(
        &self,
        host: &dyn AutoHost,
        run: &AutoRunId,
        reason: AutoEndReason,
        detail: Option<String>,
    ) -> Option<AutoRunView> {
        let now = host.now_ms();
        let mut first = false;
        let view = self.update(run, |v| {
            if v.state != AutoState::Ended {
                first = true;
                v.state = AutoState::Ended;
                v.paused = None;
                v.end_reason = Some(reason);
                v.detail = detail;
                v.ended_at = Some(now);
            }
        })?;
        if first {
            if let Some((_, control)) = self.snapshot(run) {
                control.stopped.store(true, Ordering::SeqCst);
            }
            host.ended(&view).await;
        }
        Some(view)
    }

    async fn wait(&self, control: &Control) {
        tokio::select! {
            _ = control.wake.notified() => {}
            _ = tokio::time::sleep(self.timing.poll) => {}
        }
    }

    /// The pane is still the agent the run started on, or why not.
    async fn check(
        &self,
        host: &dyn AutoHost,
        pane: &str,
        engine: &Option<String>,
    ) -> Option<AutoEndReason> {
        gone(&host.pane(pane).await, engine)
    }

    /// One `/auto/next` call with 3 retries (backoff 1 s, 3 s, 9 s) for an
    /// unreachable or busy service.
    async fn ask(
        &self,
        host: &dyn AutoHost,
        req: &AutoNextRequest,
        control: &Control,
    ) -> Result<AutoNextResponse, (AutoEndReason, Option<String>)> {
        let mut attempt = 0usize;
        loop {
            match host.next(req).await {
                Ok(resp) => return Ok(resp),
                Err(err) => match classify(&err) {
                    Failure::End(reason, detail) => return Err((reason, detail)),
                    Failure::Retry if attempt < self.timing.backoff.len() => {
                        tracing::warn!(attempt, "auto: the supervisor is unreachable; retrying");
                        let wait = self.timing.backoff[attempt];
                        attempt += 1;
                        tokio::select! {
                            _ = tokio::time::sleep(wait) => {}
                            _ = async {
                                while !control.stopped.load(Ordering::SeqCst) {
                                    control.wake.notified().await;
                                }
                            } => {}
                        }
                        if control.stopped.load(Ordering::SeqCst) {
                            return Err((AutoEndReason::UserStopped, None));
                        }
                    }
                    Failure::Retry => {
                        return Err((
                            AutoEndReason::ApiUnreachable,
                            Some("Couldn't reach Vibezeo after 3 tries.".into()),
                        ))
                    }
                },
            }
        }
    }

    async fn drive(
        self: Arc<Self>,
        host: Arc<dyn AutoHost>,
        run: AutoRunId,
        engine: Option<String>,
    ) {
        let host = &*host;
        let mut typed_on: Option<(AgentStatus, i64)> = None;
        let mut type_failures = 0u32;
        loop {
            let Some((view, control)) = self.snapshot(&run) else {
                return;
            };
            if view.state == AutoState::Ended || control.stopped.load(Ordering::SeqCst) {
                return;
            }
            let pane = view.pane_id.as_str().to_string();
            if let Some(reason) = self.check(host, &pane, &engine).await {
                self.end(host, &run, reason, None).await;
                return;
            }
            // The ceiling is held here, whatever the server answers.
            if view.turns >= view.ceiling {
                self.end(host, &run, AutoEndReason::TurnCeiling, None).await;
                return;
            }
            if view.paused == Some(AutoPause::User) {
                self.wait(&control).await;
                continue;
            }
            match readiness(host.status(&pane).as_ref(), typed_on) {
                Readiness::Approval => {
                    if view.paused != Some(AutoPause::ApprovalPending) {
                        self.update(&run, |v| {
                            v.state = AutoState::Paused;
                            v.paused = Some(AutoPause::ApprovalPending);
                        });
                    }
                    self.wait(&control).await;
                    continue;
                }
                Readiness::NotYet => {
                    if view.paused == Some(AutoPause::ApprovalPending) {
                        self.update(&run, |v| {
                            v.state = AutoState::Running;
                            v.paused = None;
                        });
                    }
                    self.wait(&control).await;
                    continue;
                }
                Readiness::Ready => {
                    if view.paused == Some(AutoPause::ApprovalPending) {
                        self.update(&run, |v| {
                            v.state = AutoState::Running;
                            v.paused = None;
                        });
                    }
                }
            }
            let req = AutoNextRequest {
                mode: view.mode.as_str().into(),
                goal: view.goal.clone(),
                turn: view.turns,
                ceiling: view.ceiling,
                scrollback: scrollback(&host.tail(&pane)),
            };
            let resp = match self.ask(host, &req, &control).await {
                Ok(resp) => resp,
                Err((reason, detail)) => {
                    self.end(host, &run, reason, detail).await;
                    return;
                }
            };
            // The world may have moved while the supervisor thought.
            if control.stopped.load(Ordering::SeqCst) {
                return;
            }
            if let Some(reason) = self.check(host, &pane, &engine).await {
                self.end(host, &run, reason, None).await;
                return;
            }
            match (resp.action.as_str(), resp.text) {
                ("prompt", Some(text)) if !text.trim().is_empty() => {
                    if control.stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    typed_on = host.status(&pane).map(|r| (r.status, r.since));
                    match host.type_prompt(&pane, &text).await {
                        Ok(()) => {
                            type_failures = 0;
                            self.update(&run, |v| v.turns += 1);
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "auto: couldn't type the prompt");
                            typed_on = None;
                            type_failures += 1;
                            if type_failures >= TYPE_FAILURES {
                                self.end(
                                    host,
                                    &run,
                                    AutoEndReason::Stuck,
                                    Some("Auto couldn't type into this pane.".into()),
                                )
                                .await;
                                return;
                            }
                        }
                    }
                }
                ("done", _) => {
                    self.end(host, &run, AutoEndReason::Done, resp.reason).await;
                    return;
                }
                ("stuck", _) => {
                    self.end(host, &run, AutoEndReason::Stuck, resp.reason)
                        .await;
                    return;
                }
                _ => {
                    self.end(
                        host,
                        &run,
                        AutoEndReason::ApiUnreachable,
                        Some("Vibezeo answered in a way this version doesn't understand.".into()),
                    )
                    .await;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI64;
    use tandem_core::status::SessionKey;

    type Answer = Box<dyn Fn(&FakeHost, usize) -> Result<AutoNextResponse, ApiError> + Send + Sync>;
    type AfterType = Box<dyn Fn(&FakeHost, usize) + Send + Sync>;

    struct FakeHost {
        clock: AtomicI64,
        pane: Mutex<PaneCheck>,
        status: Mutex<Option<StatusRecord>>,
        tail: Mutex<String>,
        /// Every PTY write, in order (the write log, F-AU-01 AC2).
        typed: Mutex<Vec<String>>,
        requests: Mutex<Vec<AutoNextRequest>>,
        answer: Answer,
        /// Called after each typed prompt with the count so far.
        after_type: AfterType,
        ended: Mutex<Vec<AutoRunView>>,
    }

    fn record(status: AgentStatus, reason: Option<WaitingReason>, since: i64) -> StatusRecord {
        StatusRecord {
            key: SessionKey::pane("pn_1"),
            status,
            reason,
            since,
            ts: since,
            tool: None,
            title: None,
            preview: None,
        }
    }

    fn prompt(text: &str) -> Result<AutoNextResponse, ApiError> {
        Ok(AutoNextResponse {
            action: "prompt".into(),
            text: Some(text.into()),
            reason: None,
        })
    }

    impl FakeHost {
        fn new(answer: Answer) -> Self {
            Self {
                clock: AtomicI64::new(1_000),
                pane: Mutex::new(PaneCheck::Agent {
                    engine_id: Some("claude".into()),
                    alive: true,
                }),
                status: Mutex::new(Some(record(AgentStatus::Idle, None, 500))),
                tail: Mutex::new("> ".into()),
                typed: Mutex::new(Vec::new()),
                requests: Mutex::new(Vec::new()),
                answer,
                after_type: Box::new(|_, _| {}),
                ended: Mutex::new(Vec::new()),
            }
        }

        fn tick(&self) -> i64 {
            self.clock.fetch_add(10, Ordering::SeqCst) + 10
        }

        fn set_status(&self, status: AgentStatus, reason: Option<WaitingReason>) {
            let since = self.tick();
            *self.status.lock().expect("lock") = Some(record(status, reason, since));
        }

        fn typed(&self) -> Vec<String> {
            self.typed.lock().expect("lock").clone()
        }
    }

    #[async_trait]
    impl AutoHost for FakeHost {
        async fn pane(&self, _pane: &str) -> PaneCheck {
            self.pane.lock().expect("lock").clone()
        }
        fn status(&self, _pane: &str) -> Option<StatusRecord> {
            self.status.lock().expect("lock").clone()
        }
        fn tail(&self, _pane: &str) -> String {
            self.tail.lock().expect("lock").clone()
        }
        async fn type_prompt(&self, _pane: &str, text: &str) -> Result<(), String> {
            let n = {
                let mut typed = self.typed.lock().expect("lock");
                typed.push(text.to_string());
                typed.len()
            };
            // The agent takes the prompt, works, and is back at its prompt.
            self.set_status(AgentStatus::Working, None);
            self.set_status(AgentStatus::Done, None);
            (self.after_type)(self, n);
            Ok(())
        }
        async fn next(&self, req: &AutoNextRequest) -> Result<AutoNextResponse, ApiError> {
            let n = {
                let mut reqs = self.requests.lock().expect("lock");
                reqs.push(req.clone());
                reqs.len()
            };
            (self.answer)(self, n)
        }
        fn now_ms(&self) -> i64 {
            self.tick()
        }
        async fn ended(&self, view: &AutoRunView) {
            self.ended.lock().expect("lock").push(view.clone());
        }
    }

    fn service() -> Arc<AutoService> {
        Arc::new(AutoService::new(
            EventBus::new(),
            AutoTiming {
                poll: Duration::from_millis(5),
                backoff: [Duration::ZERO; 3],
            },
        ))
    }

    fn start(svc: &Arc<AutoService>, host: &Arc<FakeHost>, mode: AutoMode) -> AutoRunId {
        svc.start(
            host.clone(),
            PaneId::from_db("pn_1".into()),
            Some("claude".into()),
            mode,
            None,
            None,
        )
        .expect("starts")
        .run_id
    }

    async fn until_ended(svc: &AutoService, run: &AutoRunId) -> AutoRunView {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(v) = svc.get(run) {
                    if v.state == AutoState::Ended {
                        return v;
                    }
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("the run ends")
    }

    #[tokio::test]
    async fn f_au_01_1_always_prompt_stops_at_exactly_the_finish_ceiling() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, _| {
            prompt("Continue with the next step.")
        })));
        let run = start(&svc, &host, AutoMode::Finish);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::TurnCeiling));
        assert_eq!(view.ceiling, 20);
        assert_eq!(view.turns, 20);
        assert_eq!(host.typed().len(), 20);
        assert_eq!(host.requests.lock().expect("lock").len(), 20);
        let reqs = host.requests.lock().expect("lock");
        assert_eq!(
            reqs.iter().map(|r| r.turn).collect::<Vec<_>>(),
            (0..20).collect::<Vec<_>>()
        );
        assert!(reqs.iter().all(|r| r.mode == "finish" && r.ceiling == 20));
        assert_eq!(host.ended.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn f_au_01_2_the_engine_exiting_ends_the_run_with_no_further_writes() {
        let svc = service();
        let mut host = FakeHost::new(Box::new(|_, _| prompt("next")));
        host.after_type = Box::new(|h, n| {
            if n == 3 {
                *h.pane.lock().expect("lock") = PaneCheck::Agent {
                    engine_id: Some("claude".into()),
                    alive: false,
                };
            }
        });
        let host = Arc::new(host);
        let run = start(&svc, &host, AutoMode::Finish);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::EngineExited));
        assert_eq!(host.typed().len(), 3);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(host.typed().len(), 3, "nothing is written after the exit");
    }

    #[tokio::test]
    async fn f_au_01_2_an_exit_while_the_supervisor_thinks_types_nothing() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|h, n| {
            if n == 2 {
                // The agent hands the pane back to its shell mid-call.
                *h.pane.lock().expect("lock") = PaneCheck::Shell;
            }
            prompt("next")
        })));
        let run = start(&svc, &host, AutoMode::Review);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::EngineExited));
        assert_eq!(host.typed().len(), 1);
    }

    #[tokio::test]
    async fn f_au_01_3_the_scrollback_sent_has_the_planted_key_redacted() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, _| {
            Ok(AutoNextResponse {
                action: "stuck".into(),
                text: None,
                reason: Some("Needs a person.".into()),
            })
        })));
        let key = "sk-proj-Abc123Def456Ghi789Jkl012Mno345";
        *host.tail.lock().expect("lock") = format!("$ export OPENAI_API_KEY={key}\nok\n> ");
        let run = start(&svc, &host, AutoMode::Explore);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::Stuck));
        assert_eq!(view.detail.as_deref(), Some("Needs a person."));
        let sent = host.requests.lock().expect("lock")[0].scrollback.clone();
        assert!(sent.contains("[REDACTED]"), "{sent}");
        assert!(!sent.contains(key));
    }

    #[test]
    fn the_scrollback_is_the_last_200_lines_and_at_most_16_kb() {
        let raw: String = (0..500).map(|i| format!("line {i}\n")).collect();
        let s = scrollback(&raw);
        assert!(s.starts_with("line 300"));
        assert!(s.ends_with("line 499"));
        let wide = "é".repeat(20_000);
        let s = scrollback(&wide);
        assert!(s.len() <= SCROLLBACK_BYTES);
        assert!(s.chars().all(|c| c == 'é'));
    }

    #[tokio::test]
    async fn f_au_01_4_no_credits_ends_out_of_credits_before_typing() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, _| {
            Err(ApiError::Http {
                status: 402,
                code: "CREDITS_EXHAUSTED".into(),
                detail: Some("You're out of credits until 1 November.".into()),
            })
        })));
        let run = start(&svc, &host, AutoMode::Finish);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::OutOfCredits));
        assert_eq!(
            view.detail.as_deref(),
            Some("You're out of credits until 1 November.")
        );
        assert!(host.typed().is_empty());
        assert_eq!(host.requests.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn f_au_01_5_waiting_for_approval_pauses_without_typing_then_resumes() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, n| {
            if n >= 2 {
                Ok(AutoNextResponse {
                    action: "done".into(),
                    text: None,
                    reason: Some("Verified.".into()),
                })
            } else {
                prompt("go")
            }
        })));
        host.set_status(AgentStatus::Waiting, Some(WaitingReason::Approval));
        let run = start(&svc, &host, AutoMode::Finish);
        tokio::time::timeout(Duration::from_secs(5), async {
            while svc.get(&run).and_then(|v| v.paused) != Some(AutoPause::ApprovalPending) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("pauses for the approval");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(host.typed().is_empty());
        assert!(host.requests.lock().expect("lock").is_empty());
        assert_eq!(svc.get(&run).map(|v| v.state), Some(AutoState::Paused));
        // The user answers; the agent works and comes back to its prompt.
        host.set_status(AgentStatus::Working, None);
        host.set_status(AgentStatus::Done, None);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::Done));
        assert_eq!(view.detail.as_deref(), Some("Verified."));
        assert_eq!(host.typed(), vec!["go".to_string()]);
    }

    #[tokio::test]
    async fn an_unreachable_service_is_tried_4_times_then_ends() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, _| {
            Err(ApiError::Network("offline".into()))
        })));
        let run = start(&svc, &host, AutoMode::Finish);
        let view = until_ended(&svc, &run).await;
        assert_eq!(view.end_reason, Some(AutoEndReason::ApiUnreachable));
        assert_eq!(host.requests.lock().expect("lock").len(), 4);
    }

    #[tokio::test]
    async fn plan_policy_and_sign_in_answers_name_their_end_reason() {
        for (status, code, reason) in [
            (403, "FORBIDDEN_PLAN", AutoEndReason::NoSubscription),
            (403, "FORBIDDEN_POLICY", AutoEndReason::TurnedOff),
            (401, "UNAUTHENTICATED", AutoEndReason::SignedOut),
        ] {
            let svc = service();
            let host = Arc::new(FakeHost::new(Box::new(move |_, _| {
                Err(ApiError::Http {
                    status,
                    code: code.into(),
                    detail: None,
                })
            })));
            let run = start(&svc, &host, AutoMode::Finish);
            assert_eq!(until_ended(&svc, &run).await.end_reason, Some(reason));
            assert!(host.typed().is_empty());
        }
    }

    #[tokio::test]
    async fn closing_the_pane_or_changing_the_engine_ends_the_run() {
        for (check, reason) in [
            (PaneCheck::Closed, AutoEndReason::PaneClosed),
            (
                PaneCheck::Agent {
                    engine_id: Some("codex".into()),
                    alive: true,
                },
                AutoEndReason::EngineChanged,
            ),
        ] {
            let svc = service();
            let mut host = FakeHost::new(Box::new(|_, _| prompt("next")));
            let after = Mutex::new(Some(check));
            host.after_type = Box::new(move |h, _| {
                if let Some(c) = after.lock().expect("lock").take() {
                    *h.pane.lock().expect("lock") = c;
                }
            });
            let host = Arc::new(host);
            let run = start(&svc, &host, AutoMode::Finish);
            assert_eq!(until_ended(&svc, &run).await.end_reason, Some(reason));
            assert_eq!(host.typed().len(), 1);
        }
    }

    #[tokio::test]
    async fn stop_ends_at_once_and_pause_holds_the_next_turn() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, _| prompt("next"))));
        // The agent stays busy, so the run waits.
        host.set_status(AgentStatus::Working, None);
        let run = start(&svc, &host, AutoMode::Finish);
        let paused = svc.pause(&run).expect("pauses");
        assert_eq!(paused.paused, Some(AutoPause::User));
        host.set_status(AgentStatus::Done, None);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(host.typed().is_empty(), "a paused run types nothing");
        let stopped = svc.stop(&*host, &run).await.expect("stops");
        assert_eq!(stopped.end_reason, Some(AutoEndReason::UserStopped));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(host.typed().is_empty());
        svc.dismiss(&run);
        assert!(svc.get(&run).is_none());
    }

    #[tokio::test]
    async fn one_run_per_pane_and_custom_runs_need_a_goal_and_a_ceiling_in_range() {
        let svc = service();
        let host = Arc::new(FakeHost::new(Box::new(|_, _| prompt("next"))));
        host.set_status(AgentStatus::Working, None);
        let pane = || PaneId::from_db("pn_1".into());
        let err = svc
            .start(
                host.clone(),
                pane(),
                None,
                AutoMode::Custom,
                Some("  ".into()),
                None,
            )
            .expect_err("refused");
        assert_eq!(err.code, ErrorCode::Invalid);
        let err = svc
            .start(
                host.clone(),
                pane(),
                None,
                AutoMode::Custom,
                Some("ship".into()),
                Some(31),
            )
            .expect_err("refused");
        assert_eq!(err.code, ErrorCode::Invalid);
        let view = svc
            .start(
                host.clone(),
                pane(),
                Some("claude".into()),
                AutoMode::Custom,
                Some("ship".into()),
                Some(3),
            )
            .expect("starts");
        assert_eq!(view.ceiling, 3);
        let err = svc
            .start(
                host.clone(),
                pane(),
                Some("claude".into()),
                AutoMode::Finish,
                None,
                None,
            )
            .expect_err("refused");
        assert_eq!(err.code, ErrorCode::Conflict);
    }
}
