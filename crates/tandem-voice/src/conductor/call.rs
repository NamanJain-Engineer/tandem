//! One Conductor call from start to settle (F-CN-01, ARCHITECTURE §12.6).
//!
//! - **Start.** The mic starts at once into the 3 s pre-connect buffer
//!   while `POST /v1/voice/session` mints a session (the server holds 10
//!   minutes of credits) and the talker opens and takes the call's
//!   configuration. The call is connected when the talker says Ready, and
//!   the buffer is sent then (AC4). A talker that fails before that is
//!   settled at 0 s, and the API is asked again without it (D-1017).
//!   Startup errors show; they never fall back to idle silently.
//! - **During.** Mic audio goes to the talker unless muted. Each `act`
//!   call runs the doer (`POST /v1/voice/act`, up to 8 rounds) and every
//!   tool call through the [`Executor`]; its answer goes back to the talker.
//!   A reply to a question ("answer q-… yes") is handled here, for exactly
//!   the request it names.
//! - **End.** Hang-up, sign-out, a mic that stops delivering, the time cap
//!   or 0 credits. Audio stops at once, questions are withdrawn, pending
//!   deliveries are cancelled, and nothing queued runs after (AC2). The call
//!   settles with its connected seconds (AC1); nothing pops up afterwards.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::confirm::{ConfirmError, Confirms};
use super::exec::{ExecTiming, Executor, ToolCall, ToolHost, ToolResult};
use super::memory::{self, MemoryAsk, RememberRefusal};
use super::persona::{self, PersonaBrief};
use super::view::{
    credits_for, CallError, CallErrorCode, CallPhase, CallView, ChipState, ChipView, QuestionView,
    Receipt,
};
use crate::realtime::buffer::PreConnect;
use crate::realtime::pcm::{self, Resampler};
use crate::realtime::{ClientEvent, Connection, RealtimeProvider, ServerEvent, ACT_TOOL};
use tandem_core::origin::LocalReview;

/// `POST /v1/voice/session`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGrant {
    pub session_id: String,
    pub provider: String,
    pub ephemeral_token: String,
    pub ws_url: String,
    pub expires_at: String,
    pub max_seconds: u32,
}

/// `POST /v1/voice/session`'s body.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SessionRequest {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub voice: Option<String>,
    /// The realtime protocols the call can still open; the API mints from
    /// one of them (D-1013).
    #[serde(default)]
    pub providers: Vec<String>,
}

/// `POST /v1/voice/act`'s body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActRequest {
    pub session_id: String,
    pub request: String,
    pub snapshot: Value,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub results: Vec<ToolResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActResponse {
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub say: Option<String>,
}

/// `POST /v1/voice/session/{id}/settle`'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    pub charged: u32,
    pub refunded: u32,
}

/// A hosted call that failed, by what the user can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedError {
    SignedOut,
    NeedsPro,
    /// The Team admin turned Conductor off.
    TurnedOff,
    OutOfCredits,
    RateLimited,
    Offline,
    /// 409: the server already ended the call.
    SessionEnded,
    /// 503 `VOICE_NOT_IN_REGION`: no voice provider the API has may serve
    /// the user's country, as the free Gemini may not the EEA, Switzerland
    /// or the UK (D-1018).
    NotInRegion,
    Other(String),
}

/// What the mic sends a call.
#[derive(Debug, Clone, PartialEq)]
pub enum MicEvent {
    /// Mono samples at `rate`.
    Samples(Vec<f32>, u32),
    /// "The microphone isn't delivering audio." (F-CN-08's wording; Dictate says "Your microphone…").
    NotDelivering,
}

/// The talker's voice out of the speakers.
pub trait Speaker: Send + Sync {
    fn play(&self, pcm: &[i16]);
    /// Barge-in and hang-up: silence now.
    fn stop(&self);
    fn speaking(&self) -> bool;
    fn level(&self) -> f32;
    /// Samples handed to [`Speaker::play`] so far, less any [`Speaker::stop`]
    /// dropped before they played (24 kHz; counts across calls).
    fn queued(&self) -> u64;
    /// Samples that reached the output so far. With `queued`, where the
    /// listener is in what was sent: barge-in truncates to it (D-1122).
    fn played(&self) -> u64;
}

/// The assistant item whose audio is going out: where it starts in the
/// speaker's queue, and how much of it arrived.
struct Playing {
    item_id: String,
    start: u64,
    len: u64,
}

impl Playing {
    /// While it's still playing: the item and the milliseconds of it the
    /// speaker really played (never more than arrived). `None` when
    /// nothing of it is playing: it finished, or hasn't started.
    fn heard(&self, speaker: &dyn Speaker) -> Option<(String, u32)> {
        if !speaker.speaking() {
            return None;
        }
        let played = speaker.played().saturating_sub(self.start).min(self.len);
        if played == 0 {
            return None;
        }
        let ms = played * 1000 / u64::from(crate::realtime::SAMPLE_RATE);
        Some((self.item_id.clone(), ms as u32))
    }
}

/// How a call was started (the entry points share [`CallController::start`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOrigin {
    Button,
    Palette,
    Shortcut,
    /// "Hey Conductor" (F-CN-07): it answers aloud once ready.
    Wake,
    /// Holding the ring: the mic is live only while held.
    PushToTalk,
}

/// Why a call ended (the call log's `end_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    HungUp,
    SignedOut,
    MicFailed,
    TimeCap,
    OutOfCredits,
    /// The provider hung up or the session couldn't start.
    Failed,
    /// Push-to-talk with nothing said for 2 minutes.
    Quiet,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EndReason::HungUp => "hung_up",
            EndReason::SignedOut => "signed_out",
            EndReason::MicFailed => "mic_failed",
            EndReason::TimeCap => "time_cap",
            EndReason::OutOfCredits => "out_of_credits",
            EndReason::Failed => "failed",
            EndReason::Quiet => "quiet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    User,
    Assistant,
    Action,
}

/// What the call log keeps (UI-66, `conductor_calls` and
/// `conductor_call_events`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallRecord {
    Started {
        call_id: String,
        started_at: i64,
    },
    /// Connected seconds so far, so a crash can still settle the call.
    Progress {
        call_id: String,
        seconds: u32,
    },
    Event {
        call_id: String,
        ts: i64,
        kind: LogKind,
        text: String,
        ok: Option<bool>,
    },
    Ended {
        call_id: String,
        ended_at: i64,
        seconds: u32,
        /// `None` until the API confirmed the settle.
        credits: Option<u32>,
        reason: EndReason,
    },
}

/// The app around a call; faked in tests.
#[async_trait]
pub trait CallHost: ToolHost {
    async fn mint(&self, req: &SessionRequest) -> Result<SessionGrant, HostedError>;
    async fn act(&self, req: &ActRequest) -> Result<ActResponse, HostedError>;
    async fn settle(&self, session_id: &str, seconds: u32) -> Result<Settlement, HostedError>;
    /// The workspace for the doer (no file contents) and a one-paragraph
    /// summary for the talker.
    async fn snapshot(&self) -> (Value, String);
    /// Persona and memory, read once per call (F-CN-06's seam).
    async fn persona(&self) -> PersonaBrief;
    fn provider(&self, name: &str) -> Option<Arc<dyn RealtimeProvider>>;
    /// Every name [`CallHost::provider`] opens, as the API is told them.
    fn providers(&self) -> &'static [&'static str] {
        crate::realtime::PROVIDERS
    }
    /// Claims the mic for the call (composer dictation is refused while it
    /// holds it) and starts sending audio.
    fn mic_start(&self, tx: mpsc::UnboundedSender<MicEvent>) -> Result<(), String>;
    /// Stops the mic and gives it back.
    fn mic_stop(&self);
    fn speaker(&self) -> Arc<dyn Speaker>;
    fn now_ms(&self) -> i64;
    fn emit(&self, view: &CallView);
    fn emit_level(&self, level: f32);
    async fn record(&self, record: CallRecord);
    /// "Remember that …" (F-CN-06). Refused while memory is off.
    async fn remember(&self, _fact: &str) -> Result<(), RememberRefusal> {
        Err(RememberRefusal::Off)
    }
    /// "What do you know about me": whether memory is on, and its facts.
    async fn recall(&self) -> (bool, Vec<String>) {
        (false, Vec::new())
    }
    /// "Forget …": how many facts went.
    async fn forget(&self, _query: &str) -> usize {
        0
    }
    /// After a connected call: the proposed memories for "After each call",
    /// and the journal's line (when, how long, which tools), both only while
    /// memory is on.
    async fn after_call(
        &self,
        _proposals: Vec<String>,
        _ended_at: i64,
        _seconds: u32,
        _tools: Vec<String>,
    ) {
    }
    /// A call starts, or ends: state that belongs to one call resets.
    fn call_started(&self) {}
    fn call_ended(&self) {}
    /// An `act` answered: what its tools found is being said to the user
    /// now (F-CN-09: the review item they just heard may be acted on).
    fn act_answered(&self) {}
}

#[derive(Debug, Clone)]
pub struct CallTiming {
    pub tick: Duration,
    /// How long the ended receipt shows.
    pub receipt: Duration,
    /// The warning before the cap.
    pub warn_before: Duration,
    /// Push-to-talk ends after this long without the ring held.
    pub quiet: Duration,
    /// How long a talker has to say Ready to the call's configuration.
    pub ready: Duration,
    pub exec: ExecTiming,
}

impl Default for CallTiming {
    fn default() -> Self {
        Self {
            tick: Duration::from_millis(250),
            receipt: Duration::from_secs(3),
            warn_before: Duration::from_secs(120),
            quiet: Duration::from_secs(120),
            ready: Duration::from_secs(10),
            exec: ExecTiming::default(),
        }
    }
}

/// The doer's rounds per request (the API answers in words on the 8th).
pub const MAX_ROUNDS: usize = 8;

enum Cmd {
    End(EndReason),
    Mute(bool),
    Inject(Vec<ToolCall>),
}

/// Events from a call's helpers to its loop.
enum Note {
    Chip(ChipView),
    Log(LogKind, String, Option<bool>),
    Question(Option<QuestionView>),
    Output {
        call_id: String,
        output: String,
    },
    End(EndReason, Option<CallError>),
    /// "Talk faster" (+1) or "talk slower" (-1).
    Speed(i8),
}

struct Live {
    cmd: mpsc::UnboundedSender<Cmd>,
    confirms: Arc<Confirms>,
}

/// One per app. Cheap to share.
pub struct CallController {
    host: Arc<dyn CallHost>,
    /// Calls run here, whichever thread starts them (a window's command,
    /// the wake detector).
    rt: tokio::runtime::Handle,
    timing: CallTiming,
    live: Mutex<Option<Live>>,
    view: Arc<Mutex<CallView>>,
    calls: std::sync::atomic::AtomicU64,
}

impl CallController {
    /// Must be called inside the tokio runtime calls will run on.
    pub fn new(host: Arc<dyn CallHost>, timing: CallTiming) -> Arc<Self> {
        Self::on(tokio::runtime::Handle::current(), host, timing)
    }

    pub fn on(
        rt: tokio::runtime::Handle,
        host: Arc<dyn CallHost>,
        timing: CallTiming,
    ) -> Arc<Self> {
        Arc::new(Self {
            rt,
            host,
            timing,
            live: Mutex::new(None),
            view: Arc::new(Mutex::new(CallView::default())),
            calls: Default::default(),
        })
    }

    pub fn view(&self) -> CallView {
        self.view.lock().clone()
    }

    pub fn is_live(&self) -> bool {
        self.live.lock().is_some()
    }

    /// The one entry point every way of starting a call uses (the ring, the
    /// palette, the shortcut, the wake phrase). A call already running is
    /// returned as it is.
    pub fn start(self: &Arc<Self>, origin: CallOrigin) -> CallView {
        let mut live = self.live.lock();
        if live.is_some() {
            return self.view();
        }
        let (cmd, cmds) = mpsc::unbounded_channel();
        let n = self
            .calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let (notes_tx, notes) = mpsc::unbounded_channel();
        let q_tx = notes_tx.clone();
        let confirms = Arc::new(Confirms::new(Arc::new(move |q| {
            let _ = q_tx.send(Note::Question(q));
        })));
        *live = Some(Live {
            cmd,
            confirms: confirms.clone(),
        });
        drop(live);
        let view = CallView {
            phase: CallPhase::Connecting,
            muted: origin == CallOrigin::PushToTalk,
            ..CallView::default()
        };
        *self.view.lock() = view.clone();
        self.host.emit(&view);
        let me = self.clone();
        self.rt.spawn(async move {
            me.run(origin, n, cmds, notes_tx, notes, confirms).await;
        });
        view
    }

    /// Hang up; on an ended or failed call, clear the HUD.
    pub fn end(&self) {
        self.end_with(EndReason::HungUp);
    }

    pub fn end_with(&self, reason: EndReason) {
        if let Some(live) = self.live.lock().as_ref() {
            let _ = live.cmd.send(Cmd::End(reason));
            return;
        }
        let mut v = self.view.lock();
        if matches!(v.phase, CallPhase::Ended | CallPhase::Error) {
            *v = CallView::default();
            self.host.emit(&v);
        }
    }

    pub fn mute(&self, muted: bool) {
        if let Some(live) = self.live.lock().as_ref() {
            let _ = live.cmd.send(Cmd::Mute(muted));
        }
    }

    /// The E2E harness: tool calls as if the doer had returned them, run
    /// through the same executor, chips and log. False without a call.
    pub fn inject(&self, calls: Vec<ToolCall>) -> bool {
        self.live
            .lock()
            .as_ref()
            .is_some_and(|l| l.cmd.send(Cmd::Inject(calls)).is_ok())
    }

    /// The HUD's Yes or No, clicked in the main window. `yes` carries the
    /// click's proof.
    pub fn confirm_on_screen(
        &self,
        request_id: &str,
        yes: Option<LocalReview>,
    ) -> Result<(), ConfirmError> {
        let confirms = self
            .live
            .lock()
            .as_ref()
            .map(|l| l.confirms.clone())
            .ok_or(ConfirmError::NotFound)?;
        confirms.answer_on_screen(request_id, yes)
    }

    fn set(&self, f: impl FnOnce(&mut CallView)) {
        let v = {
            let mut v = self.view.lock();
            f(&mut v);
            v.clone()
        };
        self.host.emit(&v);
    }

    async fn run(
        self: Arc<Self>,
        origin: CallOrigin,
        n: u64,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
        notes_tx: mpsc::UnboundedSender<Note>,
        mut notes: mpsc::UnboundedReceiver<Note>,
        confirms: Arc<Confirms>,
    ) {
        let host = self.host.clone();
        let cancel = CancellationToken::new();
        let mut muted = origin == CallOrigin::PushToTalk;
        let (mic_tx, mut mic) = mpsc::unbounded_channel();
        let mut buffer = PreConnect::default();
        let mut resampler: Option<(u32, Resampler)> = None;
        let mut feed = |samples: Vec<f32>, rate: u32, out: &mut Vec<i16>| {
            if resampler.as_ref().is_none_or(|(r, _)| *r != rate) {
                resampler = Some((rate, Resampler::new(rate)));
            }
            if let Some((_, r)) = resampler.as_mut() {
                r.process(&samples, out);
            }
        };

        host.call_started();
        // The mic first: early speech waits in the buffer.
        if let Err(why) = host.mic_start(mic_tx) {
            tracing::info!(error = %why, "conductor: the microphone didn't start");
            self.fail(
                None,
                CallError {
                    code: CallErrorCode::MicNotDelivering,
                    message: "The microphone isn't delivering audio.".into(),
                },
            );
            return;
        }

        // Mint and connect, while the mic fills the buffer.
        let persona = host.persona().await;
        let (_, summary) = host.snapshot().await;
        let configure = ClientEvent::Configure {
            instructions: persona::instructions(&persona, &summary),
            voice: persona.voice.clone(),
        };
        let mut connect = {
            let host = host.clone();
            let voice = persona.voice.clone();
            let (ready, stop) = (self.timing.ready, cancel.clone());
            tokio::spawn(async move { connect(host, voice, configure, ready, stop).await })
        };
        let outcome = loop {
            tokio::select! {
                done = &mut connect => break Some(done.unwrap_or(Err(Failure::other()))),
                c = cmds.recv() => match c {
                    Some(Cmd::Mute(m)) => {
                        muted = m;
                        if m { buffer.clear(); }
                        self.set(|v| v.muted = m);
                    }
                    Some(Cmd::Inject(_)) => {}
                    Some(Cmd::End(_)) | None => break None,
                },
                m = mic.recv() => match m {
                    Some(MicEvent::Samples(s, rate)) if !muted => {
                        let mut out = Vec::new();
                        feed(s, rate, &mut out);
                        buffer.push(&out);
                    }
                    Some(MicEvent::NotDelivering) => {
                        // Settle whatever was minted, then show the error.
                        cancel.cancel();
                        settle_later(host.clone(), connect);
                        host.mic_stop();
                        self.fail(None, mic_error());
                        return;
                    }
                    _ => {}
                },
            }
        };
        let (grant, conn) = match outcome {
            // Hung up while connecting: nothing pops up; a minted session
            // is settled at 0 s when it arrives.
            None => {
                buffer.clear();
                cancel.cancel();
                settle_later(host.clone(), connect);
                host.mic_stop();
                self.finish_quietly();
                return;
            }
            Some(Err(f)) => {
                buffer.clear();
                host.mic_stop();
                if let Some(id) = f.minted {
                    let _ = host.settle(&id, 0).await;
                }
                self.fail(None, f.error);
                return;
            }
            Some(Ok(ok)) => ok,
        };
        let Connection { tx, mut rx } = conn;
        let call_id = grant.session_id.clone();
        let connected_at = host.now_ms();
        host.record(CallRecord::Started {
            call_id: call_id.clone(),
            started_at: connected_at,
        })
        .await;
        if (persona.speed - 1.0).abs() > 0.01 {
            let _ = tx
                .send(ClientEvent::SetSpeed(
                    persona::SPEEDS[persona::speed_step(persona.speed)],
                ))
                .await;
        }
        // F-CN-01 AC4: early speech, after connect.
        if !buffer.is_empty() {
            let _ = tx.send(ClientEvent::Audio(buffer.take())).await;
        }
        if origin == CallOrigin::Wake {
            let _ = tx
                .send(ClientEvent::Say(
                    "The user called you with the wake phrase: say you're listening, in a few words.".into(),
                ))
                .await;
        }
        let max_ms = i64::from(grant.max_seconds) * 1000;
        self.set(|v| {
            v.call_id = Some(call_id.clone());
            v.phase = CallPhase::Listening;
            v.max_seconds = grant.max_seconds;
        });

        let executor = Arc::new(Executor::new(
            host.clone() as Arc<dyn ToolHost>,
            confirms.clone(),
            cancel.child_token(),
            self.timing.exec.clone(),
            format!("{n}"),
        ));
        let speaker = host.speaker();
        let mut ticker = tokio::time::interval(self.timing.tick);
        let mut warned = false;
        let mut last_progress = 0u32;
        let mut last_level = 0i64;
        let mut last_talk = connected_at;
        // What the user said (redacted), for post-call reflection.
        let mut said: Vec<String> = Vec::new();
        // The talker's audio item being played, for barge-in.
        let mut playing: Option<Playing> = None;
        let mut actions = 0u32;
        let mut pending_doers = 0u32;
        let mut speed = persona::speed_step(persona.speed);
        let (reason, error) = loop {
            tokio::select! {
                c = cmds.recv() => match c {
                    Some(Cmd::End(r)) => break (r, None),
                    None => break (EndReason::HungUp, None),
                    Some(Cmd::Mute(m)) => {
                        muted = m;
                        last_talk = host.now_ms();
                        self.set(|v| v.muted = m);
                    }
                    Some(Cmd::Inject(calls)) => {
                        let (exec, notes, cancel) = (executor.clone(), notes_tx.clone(), cancel.child_token());
                        let host = host.clone();
                        // A batch stands in for one act: it answers once it has run.
                        tokio::spawn(async move {
                            run_calls(&exec, calls, &cancel, &notes).await;
                            if !cancel.is_cancelled() {
                                host.act_answered();
                            }
                        });
                    }
                },
                m = mic.recv() => match m {
                    Some(MicEvent::Samples(s, rate)) => {
                        if muted { continue; }
                        let mut out = Vec::new();
                        feed(s, rate, &mut out);
                        let now = host.now_ms();
                        if now - last_level >= 100 {
                            last_level = now;
                            host.emit_level(pcm::level(&out));
                        }
                        let _ = tx.send(ClientEvent::Audio(out)).await;
                    }
                    Some(MicEvent::NotDelivering) => break (EndReason::MicFailed, Some(mic_error())),
                    None => {}
                },
                e = rx.recv() => match e {
                    Some(ServerEvent::Ready) | None => {
                        if e.is_none() {
                            break (EndReason::Failed, Some(CallError {
                                code: CallErrorCode::ConnectFailed,
                                message: "The voice service hung up.".into(),
                            }));
                        }
                    }
                    Some(ServerEvent::Closed) => break (EndReason::Failed, Some(CallError {
                        code: CallErrorCode::ConnectFailed,
                        message: "The voice service hung up.".into(),
                    })),
                    Some(ServerEvent::AudioDelta { item, pcm }) => {
                        if let Some(id) = item {
                            if playing.as_ref().map(|p| &p.item_id) != Some(&id) {
                                playing = Some(Playing { item_id: id, start: speaker.queued(), len: 0 });
                            }
                        }
                        if let Some(p) = playing.as_mut() {
                            p.len += pcm.len() as u64;
                        }
                        speaker.play(&pcm);
                        let now = host.now_ms();
                        if now - last_level >= 100 {
                            last_level = now;
                            host.emit_level(speaker.level());
                        }
                        if self.view().phase != CallPhase::Speaking {
                            self.set(|v| v.phase = CallPhase::Speaking);
                        }
                    }
                    Some(ServerEvent::SpeechStarted) => {
                        // Barge-in: stop talking at once, and tell the talker
                        // how much of its answer was heard (D-1122).
                        let heard = playing.take().and_then(|p| p.heard(speaker.as_ref()));
                        speaker.stop();
                        if let Some((item_id, audio_end_ms)) = heard {
                            let _ = tx.send(ClientEvent::Truncate { item_id, audio_end_ms }).await;
                        }
                        self.set(|v| { v.phase = CallPhase::Listening; v.caption = None; });
                    }
                    Some(ServerEvent::TranscriptUser(text)) => {
                        let text = tandem_core::redact::redact(&text);
                        said.push(text.clone());
                        host.record(CallRecord::Event { call_id: call_id.clone(), ts: host.now_ms(), kind: LogKind::User, text: text.clone(), ok: None }).await;
                        self.set(|v| v.utterance = Some(text));
                    }
                    Some(ServerEvent::TranscriptAssistant(text)) => {
                        let text = tandem_core::redact::redact(&text);
                        host.record(CallRecord::Event { call_id: call_id.clone(), ts: host.now_ms(), kind: LogKind::Assistant, text: text.clone(), ok: None }).await;
                        self.set(|v| v.caption = Some(text));
                    }
                    Some(ServerEvent::FunctionCall { call_id: fc, name, args }) => {
                        let request = args["request"].as_str().unwrap_or_default().to_string();
                        if name != ACT_TOOL || request.trim().is_empty() {
                            let _ = tx.send(ClientEvent::FunctionOutput { call_id: fc, output: "Only act is available.".into() }).await;
                            continue;
                        }
                        pending_doers += 1;
                        self.set(|v| v.phase = CallPhase::Thinking);
                        tokio::spawn(doer(
                            host.clone(),
                            executor.clone(),
                            call_id.clone(),
                            request,
                            fc,
                            cancel.child_token(),
                            notes_tx.clone(),
                        ));
                    }
                    Some(ServerEvent::ResponseDone) => {
                        if !speaker.speaking() && pending_doers == 0 {
                            self.set(|v| v.phase = CallPhase::Listening);
                        }
                    }
                    Some(ServerEvent::Error(msg)) => {
                        tracing::info!(error = %tandem_core::redact::redact(&msg), "conductor: provider error");
                    }
                },
                n = notes.recv() => match n {
                    Some(Note::Chip(chip)) => {
                        if chip.state != ChipState::Running && chip.state != ChipState::Waiting {
                            actions += 1;
                        }
                        let phase = if chip.state == ChipState::Running { CallPhase::Acting } else { CallPhase::Thinking };
                        self.set(|v| {
                            match v.chips.iter_mut().find(|c| c.id == chip.id) {
                                Some(c) => *c = chip,
                                None => v.chips.push(chip),
                            }
                            v.phase = phase;
                        });
                    }
                    Some(Note::Log(kind, text, ok)) => {
                        host.record(CallRecord::Event { call_id: call_id.clone(), ts: host.now_ms(), kind, text, ok }).await;
                    }
                    Some(Note::Question(q)) => {
                        if let Some(q) = &q {
                            let ask = format!(
                                "Ask the user: {} (request id {}).{}",
                                q.summary,
                                q.request_id,
                                if q.review == tandem_core::voice_tools::Review::ScreenOnly {
                                    " It needs a click on screen; a spoken yes can't confirm it."
                                } else { "" }
                            );
                            let _ = tx.send(ClientEvent::Say(ask)).await;
                        }
                        self.set(|v| v.question = q);
                    }
                    Some(Note::Output { call_id: fc, output }) => {
                        pending_doers = pending_doers.saturating_sub(1);
                        let _ = tx.send(ClientEvent::FunctionOutput { call_id: fc, output }).await;
                        if pending_doers == 0 {
                            self.set(|v| if matches!(v.phase, CallPhase::Thinking | CallPhase::Acting) { v.phase = CallPhase::Listening });
                        }
                    }
                    Some(Note::End(r, e)) => break (r, e),
                    Some(Note::Speed(delta)) => {
                        speed = (speed as i64 + i64::from(delta)).clamp(0, persona::SPEEDS.len() as i64 - 1) as usize;
                        let _ = tx.send(ClientEvent::SetSpeed(persona::SPEEDS[speed])).await;
                    }
                    None => {}
                },
                _ = ticker.tick() => {
                    let now = host.now_ms();
                    let elapsed = (now - connected_at).max(0);
                    let seconds = (elapsed / 1000) as u32;
                    if elapsed >= max_ms {
                        break (EndReason::TimeCap, Some(CallError {
                            code: CallErrorCode::TimeCap,
                            message: "This call reached its 30-minute limit.".into(),
                        }));
                    }
                    if !warned && elapsed >= max_ms - self.timing.warn_before.as_millis() as i64 {
                        warned = true;
                        let _ = tx.send(ClientEvent::Say("Tell the user: two minutes are left in this call.".into())).await;
                        self.set(|v| v.notice = Some("2 minutes left in this call".into()));
                    }
                    if origin == CallOrigin::PushToTalk && muted && now - last_talk >= self.timing.quiet.as_millis() as i64 {
                        break (EndReason::Quiet, None);
                    }
                    if seconds >= last_progress + 10 {
                        last_progress = seconds;
                        host.record(CallRecord::Progress { call_id: call_id.clone(), seconds }).await;
                    }
                    if self.view().phase == CallPhase::Speaking && !speaker.speaking() {
                        self.set(|v| v.phase = CallPhase::Listening);
                    }
                    if self.view().elapsed_ms / 1000 != elapsed / 1000 {
                        self.set(|v| v.elapsed_ms = elapsed);
                    }
                },
            }
        };

        // The end: everything stops before anything else happens.
        cancel.cancel();
        confirms.cancel_all();
        speaker.stop();
        host.mic_stop();
        host.call_ended();
        drop(tx);
        let ended_at = host.now_ms();
        let seconds =
            ((ended_at - connected_at).max(0) / 1000).min(i64::from(grant.max_seconds)) as u32;
        let credits = match host.settle(&call_id, seconds).await {
            Ok(s) => Some(s.charged),
            Err(e) => {
                tracing::warn!(?e, "conductor: settle failed; retried at the next start");
                None
            }
        };
        host.record(CallRecord::Ended {
            call_id: call_id.clone(),
            ended_at,
            seconds,
            credits,
            reason,
        })
        .await;
        let tools: Vec<String> = self.view().chips.iter().map(|c| c.tool.clone()).collect();
        host.after_call(memory::proposals(&said), ended_at, seconds, tools)
            .await;
        let receipt = Receipt {
            call_id: call_id.clone(),
            seconds,
            credits: credits.unwrap_or_else(|| credits_for(seconds)),
            actions,
        };
        *self.live.lock() = None;
        self.set(|v| {
            v.phase = if error.is_some() {
                CallPhase::Error
            } else {
                CallPhase::Ended
            };
            v.question = None;
            v.elapsed_ms = i64::from(seconds) * 1000;
            v.notice = if reason == EndReason::Quiet {
                Some("Call ended after 2 quiet minutes".into())
            } else {
                None
            };
            v.error = error.clone();
            v.receipt = Some(receipt);
        });
        if error.is_none() {
            self.clear_after_receipt();
        }
    }

    /// Shows a startup or call failure; it stays until dismissed.
    fn fail(&self, call_id: Option<String>, error: CallError) {
        *self.live.lock() = None;
        self.set(|v| {
            *v = CallView {
                call_id,
                phase: CallPhase::Error,
                error: Some(error),
                ..CallView::default()
            };
        });
    }

    fn finish_quietly(&self) {
        *self.live.lock() = None;
        self.set(|v| *v = CallView::default());
    }

    fn clear_after_receipt(self: &Arc<Self>) {
        let me = self.clone();
        let wait = self.timing.receipt;
        self.rt.spawn(async move {
            tokio::time::sleep(wait).await;
            if me.live.lock().is_none() && me.view().phase == CallPhase::Ended {
                me.set(|v| *v = CallView::default());
            }
        });
    }
}

fn mic_error() -> CallError {
    CallError {
        code: CallErrorCode::MicNotDelivering,
        message: "The microphone isn't delivering audio.".into(),
    }
}

struct Failure {
    error: CallError,
    /// A session was minted and needs settling.
    minted: Option<String>,
}

impl Failure {
    fn other() -> Self {
        Self {
            error: CallError {
                code: CallErrorCode::ConnectFailed,
                message: "Couldn't connect to Conductor.".into(),
            },
            minted: None,
        }
    }
}

fn hosted_error(e: &HostedError) -> CallError {
    let (code, message) = match e {
        HostedError::SignedOut => (CallErrorCode::SignedOut, "Sign in to use Conductor."),
        HostedError::NeedsPro => (
            CallErrorCode::NeedsPro,
            "Conductor comes with Vibezeo accounts.",
        ),
        HostedError::TurnedOff => (
            CallErrorCode::TurnedOff,
            "Your organization turned Conductor off.",
        ),
        HostedError::OutOfCredits => (CallErrorCode::OutOfCredits, "Out of credits."),
        HostedError::RateLimited => (
            CallErrorCode::RateLimited,
            "Too many calls just now. Try again in a few minutes.",
        ),
        HostedError::Offline => (
            CallErrorCode::Offline,
            "You're offline. Conductor needs a connection.",
        ),
        HostedError::NotInRegion => (
            CallErrorCode::NotInRegion,
            "Conductor's voice isn't available in your country yet.",
        ),
        HostedError::SessionEnded | HostedError::Other(_) => (
            CallErrorCode::ConnectFailed,
            "Couldn't connect to Conductor.",
        ),
    };
    CallError {
        code,
        message: message.into(),
    }
}

/// Mints a session and opens its talker with the call's configuration:
/// connected once the talker says [`ServerEvent::Ready`]. A talker that
/// fails first (refused, closed or silent at its setup, as a free tier over
/// its limits may be) is settled at 0 s and left out of the next ask, until
/// none is left or the call was hung up (D-1017).
async fn connect(
    host: Arc<dyn CallHost>,
    voice: Option<String>,
    configure: ClientEvent,
    ready_within: Duration,
    stop: CancellationToken,
) -> Result<(SessionGrant, Connection), Failure> {
    let mut req = SessionRequest {
        voice,
        providers: host.providers().iter().map(|p| p.to_string()).collect(),
    };
    loop {
        let grant = host.mint(&req).await.map_err(|e| Failure {
            error: hosted_error(&e),
            minted: None,
        })?;
        let failed = Failure {
            minted: Some(grant.session_id.clone()),
            ..Failure::other()
        };
        // An API from before D-1013 ignores the list: it can answer with a
        // provider the call already left out.
        if !req.providers.contains(&grant.provider) {
            tracing::warn!(provider = %grant.provider, "conductor: a session from a provider the call can't open");
            return Err(failed);
        }
        if let Some(conn) = open(host.as_ref(), &grant, &configure, ready_within).await {
            return Ok((grant, conn));
        }
        req.providers.retain(|p| *p != grant.provider);
        if req.providers.is_empty() {
            return Err(failed);
        }
        let _ = host.settle(&grant.session_id, 0).await;
        if stop.is_cancelled() {
            return Err(Failure::other());
        }
    }
}

/// Opens the grant's talker and configures it: the connection once the
/// talker says Ready, `None` if it was refused, closed first or not ready in
/// time. Its errors until then are logged, redacted.
async fn open(
    host: &dyn CallHost,
    grant: &SessionGrant,
    configure: &ClientEvent,
    within: Duration,
) -> Option<Connection> {
    let provider = &grant.provider;
    let Some(client) = host.provider(provider) else {
        tracing::warn!(%provider, "conductor: no client for this voice provider");
        return None;
    };
    let mut conn = match client.connect(&grant.ws_url, &grant.ephemeral_token).await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::info!(%provider, error = %e, "conductor: connect failed");
            return None;
        }
    };
    if conn.tx.send(configure.clone()).await.is_err() {
        return None;
    }
    let ready = async {
        loop {
            match conn.rx.recv().await {
                Some(ServerEvent::Ready) => return true,
                Some(ServerEvent::Closed) | None => return false,
                Some(ServerEvent::Error(msg)) => tracing::info!(
                    %provider,
                    error = %tandem_core::redact::redact(&msg),
                    "conductor: the talker failed at its setup"
                ),
                // Every provider says Ready before anything else.
                Some(_) => {}
            }
        }
    };
    let ready = tokio::time::timeout(within, ready).await;
    match ready {
        Ok(true) => Some(conn),
        Ok(false) => {
            tracing::info!(%provider, "conductor: the talker closed before it was ready");
            None
        }
        Err(_) => {
            tracing::info!(%provider, "conductor: the talker wasn't ready in time");
            None
        }
    }
}

/// Hung up (or the mic failed) while connecting: when the connect finishes,
/// close it and settle the session at 0 s, so its hold comes back.
fn settle_later(
    host: Arc<dyn CallHost>,
    handle: tokio::task::JoinHandle<Result<(SessionGrant, Connection), Failure>>,
) {
    tokio::spawn(async move {
        let minted = match handle.await {
            Ok(Ok((grant, conn))) => {
                drop(conn);
                Some(grant.session_id)
            }
            Ok(Err(f)) => f.minted,
            Err(_) => None,
        };
        if let Some(id) = minted {
            let _ = host.settle(&id, 0).await;
        }
    });
}

/// "Talk faster" / "speak slower" / "slow down" / "speed up": +1 or -1.
pub fn parse_speed(request: &str) -> Option<i8> {
    let r = request.to_lowercase();
    let r = r.trim().trim_end_matches(['.', '!', '?']);
    let talk = ["talk", "speak", "go"];
    for verb in talk {
        if r == format!("{verb} faster") || r == format!("{verb} a bit faster") {
            return Some(1);
        }
        if r == format!("{verb} slower") || r == format!("{verb} a bit slower") {
            return Some(-1);
        }
    }
    match r {
        "speed up" | "faster please" => Some(1),
        "slow down" | "slower please" => Some(-1),
        _ => None,
    }
}

/// "answer q-1-2 yes", or a bare yes or no for the question shown now.
pub fn parse_answer(request: &str, head: Option<&QuestionView>) -> Option<(String, bool)> {
    let words: Vec<String> = request
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let yes_no = |w: &str| match w {
        "yes" | "allow" | "confirm" | "approve" | "ok" | "okay" => Some(true),
        "no" | "deny" | "cancel" | "reject" => Some(false),
        _ => None,
    };
    match words.as_slice() {
        [a, id, answer, ..] if a == "answer" => Some((id.clone(), yes_no(answer)?)),
        [answer] => Some((head?.request_id.clone(), yes_no(answer)?)),
        _ => None,
    }
}

/// One `act` request: local answers first, else up to 8 doer rounds, each
/// tool call through the executor. The answer goes back to the talker.
async fn doer(
    host: Arc<dyn CallHost>,
    exec: Arc<Executor>,
    session_id: String,
    request: String,
    fc: String,
    cancel: CancellationToken,
    notes: mpsc::UnboundedSender<Note>,
) {
    let output = doer_rounds(&host, &exec, &session_id, &request, &cancel, &notes).await;
    if !cancel.is_cancelled() && !output.is_empty() {
        host.act_answered();
    }
    let _ = notes.send(Note::Output {
        call_id: fc,
        output: tandem_core::redact::redact(&output),
    });
}

async fn doer_rounds(
    host: &Arc<dyn CallHost>,
    exec: &Arc<Executor>,
    session_id: &str,
    request: &str,
    cancel: &CancellationToken,
    notes: &mpsc::UnboundedSender<Note>,
) -> String {
    if let Some(delta) = parse_speed(request) {
        let _ = notes.send(Note::Speed(delta));
        return if delta > 0 {
            "Okay, I'll talk faster.".into()
        } else {
            "Okay, I'll talk slower.".into()
        };
    }
    if let Some((id, yes)) = parse_answer(request, exec.confirms().head().as_ref()) {
        return match exec.confirms().answer_by_voice(&id, yes) {
            Ok(()) => "Got it.".into(),
            Err(ConfirmError::NeedsScreen) => {
                "That one needs a click on screen; a spoken yes can't confirm it.".into()
            }
            Err(ConfirmError::NotFound) => "There's no question waiting with that id.".into(),
        };
    }
    if let Some(ask) = memory::parse_memory(request) {
        return match ask {
            MemoryAsk::Remember(fact) => {
                let result = match memory::check_fact(&fact) {
                    Ok(()) => host.remember(&fact).await,
                    Err(refused) => Err(refused),
                };
                memory::remembered_reply(result)
            }
            MemoryAsk::Recall => {
                let (on, facts) = host.recall().await;
                memory::recall_reply(on, &facts)
            }
            MemoryAsk::Forget(query) => memory::forgot_reply(&query, host.forget(&query).await),
        };
    }
    let mut results: Vec<ToolResult> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        if cancel.is_cancelled() {
            return String::new();
        }
        let (snapshot, _) = host.snapshot().await;
        let req = ActRequest {
            session_id: session_id.to_string(),
            request: request.to_string(),
            snapshot,
            results: results.clone(),
        };
        let resp = match host.act(&req).await {
            Ok(r) => r,
            Err(HostedError::OutOfCredits) => {
                let _ = notes.send(Note::End(
                    EndReason::OutOfCredits,
                    Some(hosted_error(&HostedError::OutOfCredits)),
                ));
                return String::new();
            }
            Err(HostedError::SignedOut) => {
                let _ = notes.send(Note::End(
                    EndReason::SignedOut,
                    Some(hosted_error(&HostedError::SignedOut)),
                ));
                return String::new();
            }
            Err(HostedError::SessionEnded) => {
                let _ = notes.send(Note::End(EndReason::TimeCap, None));
                return String::new();
            }
            Err(e) => {
                tracing::info!(?e, "conductor: the planner failed");
                return "I couldn't reach the planner just now.".into();
            }
        };
        if resp.tool_calls.is_empty() {
            return resp.say.unwrap_or_else(|| "Done.".into());
        }
        results.extend(run_calls(exec, resp.tool_calls, cancel, notes).await);
        if cancel.is_cancelled() {
            return String::new();
        }
    }
    "Done.".into()
}

/// Runs the doer's calls in order, each as a chip and a log line.
async fn run_calls(
    exec: &Arc<Executor>,
    calls: Vec<ToolCall>,
    cancel: &CancellationToken,
    notes: &mpsc::UnboundedSender<Note>,
) -> Vec<ToolResult> {
    let mut results = Vec::new();
    for call in calls {
        if cancel.is_cancelled() {
            break;
        }
        let spec = tandem_core::voice_tools::find(&call.name);
        let mut chip = ChipView {
            id: call.id.clone(),
            tool: call.name.clone(),
            group: spec
                .map(|s| s.group)
                .unwrap_or(tandem_core::voice_tools::VoiceToolGroup::App),
            label: exec.label(&call).await,
            state: if spec.is_some_and(|s| s.review != tandem_core::voice_tools::Review::None) {
                ChipState::Waiting
            } else {
                ChipState::Running
            },
            reason: None,
        };
        let _ = notes.send(Note::Chip(chip.clone()));
        let ran = exec.run(&call).await;
        chip.state = if ran.result.ok {
            ChipState::Done
        } else {
            ChipState::Failed
        };
        chip.reason = (!ran.result.ok).then(|| ran.result.output.clone());
        let _ = notes.send(Note::Chip(chip.clone()));
        let mut text = format!("{}: {}", chip.label, ran.result.output);
        if let Some(how) = ran.confirmed {
            text = format!("{text} ({how})");
        }
        let _ = notes.send(Note::Log(LogKind::Action, text, Some(ran.result.ok)));
        results.push(ran.result);
    }
    results
}

#[cfg(test)]
mod tests;
