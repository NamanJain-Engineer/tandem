//! The Dictate controller: hotkey gestures → the mic → speech-to-text → the
//! dictionary and clean-up → insertion (F-DI-01, F-DI-06, F-DI-12;
//! ARCHITECTURE §12). It owns the take state machine, so whatever arrives
//! late or for a replaced take is dropped, and it talks to the app only
//! through `DictateSink`, so all of it runs in tests with fakes.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tandem_core::ids::TakeId;
use ts_rs::TS;

use crate::capture::{AudioService, CaptureError, CaptureEvent};
use crate::chimes::{Chime, ChimePlayer};
use crate::cloud::{
    CloudRequest, CloudStt, EngineChoice, RewriteMode, Rewriter, ToastAction, REWRITE_MAX_CHARS,
};
use crate::dictionary::{Dictionary, Rule};
use crate::guards::{self, Verdict};
use crate::hotkey::{
    Action, Binding, Bindings, Gesture, HotkeyBackend, HotkeyStatus, KeyEvent, Phase, Verifier,
    Which,
};
use crate::insert::{self, CopyReason, Outcome, Output, PendingRestore, Platform, Target};
use crate::media::PauseMedia;
use crate::postproc;
use crate::stt::{EngineSlot, SttOptions};
use crate::take::{Origin, TakeMachine, TakeState};

/// What the widget and the composers show (`dictate:state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DictatePhase {
    Idle,
    Listening,
    Transcribing,
    Inserted,
    Copied,
    Error,
    Reconnecting,
    /// A rewrite is running (F-DI-10): "Enhancing…" / "Polishing…".
    Enhancing,
    Polishing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DictateStateEvent {
    pub state: DictatePhase,
    pub take_id: Option<TakeId>,
    /// Latched or toggled: "Hands-free · press {key} to stop".
    pub hands_free: bool,
    /// A composer take's target (its own button shows the ring).
    pub composer: Option<String>,
    /// Inserted: the word count.
    pub words: Option<u32>,
}

impl DictateStateEvent {
    fn new(state: DictatePhase, take_id: Option<TakeId>) -> Self {
        Self {
            state,
            take_id,
            hands_free: false,
            composer: None,
            words: None,
        }
    }
}

/// A one-line message, rendered from `code` by the UI's catalog; `detail`
/// is the full diagnostic for the tooltip (never transcript text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DictateToast {
    pub code: ToastCode,
    pub error: bool,
    /// `{app}` for the elevated warning.
    pub app: Option<String>,
    /// `{model}` (and `{previous}`) for model messages.
    pub model: Option<String>,
    pub previous: Option<String>,
    pub detail: String,
    /// The line itself when it comes from the server (a cloud limit).
    pub message: Option<String>,
    /// What the toast offers ("Use local for this take").
    pub actions: Vec<ToastAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToastCode {
    NoSpeech,
    TooShort,
    Copied,
    Elevated,
    NoMicrophone,
    MicPermission,
    MicSilent,
    SwitchedToBackup,
    NoModel,
    HotkeyFailed,
    SecureInput,
    TranscribeFailed,
    ConductorListening,
    TimeAlmostUp,
    /// "Couldn't load {model}. Still using {previous}."
    ModelFailed,
    /// "Using {model} because {previous} isn't downloaded."
    ModelFallback,
    /// "Microphone stopped responding" (reconnecting).
    Reconnecting,
    /// A cloud take failed: `message` says why (F-DI-05).
    CloudFailed,
    /// "That's too long to enhance (5,000 characters max)." Nothing was sent.
    TooLongToEnhance,
    /// "Enhance didn't work, so your exact words were used."
    EnhanceFailed,
    /// A Polish (or a formatting run, which is one) failed.
    PolishFailed,
}

/// A finished take for the history (F-DI-08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedTake {
    pub id: TakeId,
    pub text: String,
    pub model: String,
    pub word_count: u32,
    pub duration_ms: u64,
    pub app_id: Option<String>,
}

/// Everything the controller needs from the app.
pub trait DictateSink: Send + Sync {
    fn state(&self, event: DictateStateEvent);
    fn toast(&self, toast: DictateToast);
    /// A composer take's text: inserted at that composer's caret.
    fn composer_insert(&self, target_id: &str, text: &str);
    fn finished(&self, take: FinishedTake);
    /// The dictionary rules (the controller filters by language and app).
    fn rules(&self) -> Vec<Rule>;
    /// The focused composer, if Tandem's main window has focus (F-DI-12).
    fn composer_focus(&self) -> Option<String>;
    /// The verify test's raw key events (`dictate:key`).
    fn key(&self, phase: Phase, native: bool);
    /// Verification passed.
    fn verified(&self);
    /// A model switch held back by a take can run now.
    fn switch_due(&self, model_id: String);
    /// The global shortcut's status changed on its own: secure input
    /// paused it, or its session died (F-DI-02).
    fn hotkey_status(&self, _status: HotkeyStatus) {}
    /// The verify test ran [`FN_GRACE`] with Fn alone as the hold key and
    /// heard nothing from it: some keyboards and macOS settings never
    /// deliver Fn (F-DI-02). Once per verify test.
    fn fn_unheard(&self) {}
}

/// How long the verify test waits for Fn before Dictate offers its fallback.
pub const FN_GRACE: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct DictateConfig {
    pub enabled: bool,
    pub output: Output,
    pub sounds: bool,
    pub pause_media: bool,
    /// `None` = detect.
    pub language: Option<String>,
    pub latch: bool,
    /// Local or Cloud (F-DI-05); Cloud needs `DictateDeps::cloud`.
    pub engine: EngineChoice,
    /// "Share dictionary hints with cloud".
    pub share_hints: bool,
    /// "Enhance every take automatically" (F-DI-10).
    pub auto_enhance: bool,
    /// Formatting instructions for every take, and per app (app id, text).
    pub formatting: Option<String>,
    pub formatting_apps: Vec<(String, String)>,
}

impl DictateConfig {
    /// The instructions for a take in `app`: its own, else the general ones.
    pub fn instructions_for(&self, app: Option<&str>) -> Option<String> {
        app.and_then(|a| {
            self.formatting_apps
                .iter()
                .find(|(id, _)| id.eq_ignore_ascii_case(a))
                .map(|(_, t)| t.clone())
        })
        .or_else(|| self.formatting.clone())
        .filter(|t| !t.trim().is_empty())
    }
}

impl Default for DictateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            output: Output::Paste,
            sounds: true,
            pause_media: false,
            language: None,
            latch: true,
            engine: EngineChoice::Local,
            share_hints: false,
            auto_enhance: false,
            formatting: None,
            formatting_apps: Vec::new(),
        }
    }
}

pub struct DictateDeps {
    pub audio: Arc<AudioService>,
    pub hotkeys: Arc<dyn HotkeyBackend>,
    pub platform: Arc<dyn Platform>,
    pub engine: Arc<EngineSlot>,
    pub chimes: Arc<dyn ChimePlayer>,
    pub media: Option<Arc<PauseMedia>>,
    pub sink: Arc<dyn DictateSink>,
    /// The hosted transcription service (F-DI-05), when there is one.
    pub cloud: Option<Arc<dyn CloudStt>>,
    /// The hosted rewrite (F-DI-10), when there is one.
    pub rewriter: Option<Arc<dyn Rewriter>>,
}

/// The last take typed into another app, for the quick actions (F-DI-10).
#[derive(Clone)]
struct LastTake {
    text: String,
    target: Option<Target>,
    app: Option<String>,
    pasted: bool,
    config: DictateConfig,
}

/// A cloud take that failed, kept so "Use local for this take" can run it
/// on this device. A new take drops it.
struct Retry {
    origin: Origin,
    target: Option<Target>,
    samples: Vec<f32>,
    end: usize,
    duration: Duration,
    config: DictateConfig,
}

struct Current {
    id: TakeId,
    target: Option<Target>,
    started: Instant,
    shown: bool,
    hands_free: bool,
    warned: bool,
}

struct Inner {
    machine: TakeMachine,
    gesture: Gesture,
    current: Option<Current>,
    config: DictateConfig,
    verifier: Option<Verifier>,
    restore: Option<PendingRestore>,
    mic_owner: Option<String>,
    retry: Option<Retry>,
    last: Option<LastTake>,
    /// The backend's status as last seen, and when it was checked (ms).
    hotkey: (HotkeyStatus, u64),
    /// The hold key is Fn alone (macOS's default).
    hold_is_fn: bool,
    /// When the verify test started, while nothing from the hold key has
    /// been heard in it; cleared once Fn is reported unheard.
    verify_since: Option<Instant>,
}

/// Cheap to clone; every clone drives the same controller.
#[derive(Clone)]
pub struct Dictate {
    inner: Arc<Mutex<Inner>>,
    deps: Arc<DictateDeps>,
    stop: Arc<AtomicBool>,
    /// Bumped by each `start_hotkeys`; an older clock thread stops.
    generation: Arc<AtomicU64>,
}

/// How long the widget waits before "No speech detected" for an empty take
/// is decided: never; guards run on the finished recording.
pub const COPIED_FOR: Duration = Duration::from_secs(4);

impl Dictate {
    pub fn new(deps: DictateDeps, config: DictateConfig) -> Self {
        let latch = config.latch;
        Self {
            inner: Arc::new(Mutex::new(Inner {
                machine: TakeMachine::new(),
                gesture: Gesture::new(latch),
                current: None,
                config,
                verifier: None,
                restore: None,
                mic_owner: None,
                retry: None,
                last: None,
                hotkey: (HotkeyStatus::NotRunning, 0),
                hold_is_fn: false,
                verify_since: None,
            })),
            deps: Arc::new(deps),
            stop: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Starts listening for the shortcut and runs the gesture clock.
    pub fn start_hotkeys(&self, bindings: &Bindings) -> HotkeyStatus {
        let (tx, rx) = mpsc::channel::<KeyEvent>();
        let status = match self.deps.hotkeys.start(bindings, tx) {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(error = %err, "dictate: hotkeys unavailable");
                self.deps.hotkeys.status()
            }
        };
        {
            let mut i = self.inner.lock();
            i.hotkey = (status.clone(), crate::hotkey::now_ms());
            i.hold_is_fn = bindings.hold.as_ref().is_some_and(Binding::is_fn_alone);
        }
        // One clock per start: a restart retires the previous thread.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("dictate-keys".into())
            .spawn(move || loop {
                if me.stop.load(Ordering::Acquire)
                    || me.generation.load(Ordering::Acquire) != generation
                {
                    break;
                }
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(e) => me.on_key(e),
                    Err(mpsc::RecvTimeoutError::Timeout) => me.tick(generation),
                    // The backend never started or went away: no keys come,
                    // but takes from the mic button and the deep link still
                    // need the clock (the 10-minute cap and its warning).
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(Duration::from_millis(10));
                        me.tick(generation);
                    }
                }
            });
        status
    }

    /// Stops the shortcut to start it again (a new binding): the clock
    /// thread retires first, so the gap isn't reported as a failure.
    pub fn stop_hotkeys(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.deps.hotkeys.stop();
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        self.deps.hotkeys.stop();
        self.cancel();
    }

    pub fn set_config(&self, config: DictateConfig) {
        let mut i = self.inner.lock();
        i.gesture.set_latch(config.latch);
        i.config = config;
    }

    pub fn config(&self) -> DictateConfig {
        self.inner.lock().config.clone()
    }

    pub fn state(&self) -> TakeState {
        self.inner.lock().machine.state()
    }

    pub fn current_take(&self) -> Option<TakeId> {
        self.inner.lock().machine.current().cloned()
    }

    /// Tests: makes the running take `by` older, for the 10-minute cap.
    #[cfg(test)]
    fn age_take(&self, by: Duration) {
        if let Some(c) = self.inner.lock().current.as_mut() {
            c.started -= by;
        }
    }

    /// Conductor (M16) claims the mic: composer dictation is refused.
    pub fn set_mic_owner(&self, owner: Option<String>) {
        self.inner.lock().mic_owner = owner;
    }

    /// "Press your shortcut now": raw events go to the verifier, not to takes.
    pub fn verify_start(&self) {
        let mut i = self.inner.lock();
        i.verifier = Some(Verifier::default());
        i.verify_since = Some(Instant::now());
    }

    pub fn verify_stop(&self) {
        let mut i = self.inner.lock();
        i.verifier = None;
        i.verify_since = None;
    }

    /// Tests: makes the running verify test `by` older, for [`FN_GRACE`].
    #[cfg(test)]
    fn age_verify(&self, by: Duration) {
        if let Some(t) = self.inner.lock().verify_since.as_mut() {
            *t -= by;
        }
    }

    /// Fn alone is the hold key and the verify test has heard nothing from
    /// it for [`FN_GRACE`]: tell the app, once.
    fn watch_fn(&self) {
        let due = {
            let mut i = self.inner.lock();
            let due = i.hold_is_fn
                && i.verifier.is_some()
                && i.verify_since.is_some_and(|t| t.elapsed() >= FN_GRACE);
            if due {
                i.verify_since = None;
            }
            due
        };
        if due {
            self.deps.sink.fn_unheard();
        }
    }

    /// The running take's phase while it records: Reconnecting during the
    /// silent-stream recovery (F-DI-03, F-DI-07), Listening once sound is
    /// back. False when no shown take is recording.
    fn recording_phase(&self, phase: DictatePhase) -> bool {
        let event = {
            let i = self.inner.lock();
            let Some(c) = i.current.as_ref().filter(|c| c.shown) else {
                return false;
            };
            if i.machine.current() != Some(&c.id) || i.machine.state() != TakeState::Recording {
                return false;
            }
            let mut e = DictateStateEvent::new(phase, Some(c.id.clone()));
            e.hands_free = c.hands_free;
            e.composer = match i.machine.origin(&c.id) {
                Some(Origin::Composer { target_id }) => Some(target_id.clone()),
                _ => None,
            };
            e
        };
        self.emit(event);
        true
    }

    pub fn on_capture_event(&self, event: CaptureEvent) {
        match event {
            CaptureEvent::Rebuilt => {
                if self.recording_phase(DictatePhase::Reconnecting) {
                    self.toast(
                        ToastCode::Reconnecting,
                        false,
                        "The microphone was silent; reopening it.".into(),
                    );
                }
            }
            CaptureEvent::Resumed => {
                self.recording_phase(DictatePhase::Listening);
            }
            CaptureEvent::SwitchedToBackup { device } => {
                self.recording_phase(DictatePhase::Listening);
                self.toast(
                    ToastCode::SwitchedToBackup,
                    false,
                    format!(
                        "The primary microphone stopped delivering audio; now recording from {device}."
                    ),
                )
            }
            CaptureEvent::NotDelivering => {
                self.toast(
                    ToastCode::MicSilent,
                    true,
                    "No samples arrived within 1 s, after rebuilding the stream and trying the backup microphone.".into(),
                );
                self.cancel();
            }
        }
    }

    fn on_key(&self, e: KeyEvent) {
        {
            let mut i = self.inner.lock();
            if let Some(v) = i.verifier.as_mut() {
                if e.which != Which::Esc {
                    let passed = v.on(&e);
                    if e.which == Which::Hold && e.native {
                        i.verify_since = None;
                    }
                    drop(i);
                    self.deps.sink.key(e.phase, e.native);
                    if passed {
                        self.inner.lock().verifier = None;
                        self.deps.sink.verified();
                    }
                }
                return;
            }
        }
        let actions = self.inner.lock().gesture.on(e);
        for a in actions {
            self.act(a, None);
        }
    }

    /// One beat of the clock thread started as `generation`.
    fn tick(&self, generation: u64) {
        let now = crate::hotkey::now_ms();
        let actions = self.inner.lock().gesture.tick(now);
        for a in actions {
            self.act(a, None);
        }
        self.watch_hotkeys(now, generation);
        self.watch_fn();
        // The 10-minute cap, and the warning 30 s before it.
        let (stop, warn) = {
            let mut i = self.inner.lock();
            match i.current.as_mut() {
                Some(c) if c.shown => {
                    let el = c.started.elapsed();
                    let warn = el >= guards::WARN_AT && !c.warned;
                    if warn {
                        c.warned = true;
                    }
                    (el >= guards::MAX_TAKE, warn)
                }
                _ => (false, false),
            }
        };
        if warn {
            self.toast(
                ToastCode::TimeAlmostUp,
                false,
                "A take is at most 10 minutes long.".into(),
            );
        }
        if stop {
            self.inner.lock().gesture.reset();
            self.act(Action::Stop, None);
        }
    }

    /// Every 250 ms: has the backend's status changed on its own? Secure
    /// input pausing it says so; a working shortcut that stops says so.
    fn watch_hotkeys(&self, now: u64, generation: u64) {
        let old = {
            let mut i = self.inner.lock();
            if now.saturating_sub(i.hotkey.1) < 250 {
                return;
            }
            i.hotkey.1 = now;
            i.hotkey.0.clone()
        };
        let new = self.deps.hotkeys.status();
        // The app restarted the shortcut (a new binding) while this thread
        // read it: what it read is the gap between stop and start, not a
        // failure, and the restart reports the new status itself.
        if new == old || self.generation.load(Ordering::Acquire) != generation {
            return;
        }
        self.inner.lock().hotkey.0 = new.clone();
        self.deps.sink.hotkey_status(new.clone());
        let working = matches!(old, HotkeyStatus::Active | HotkeyStatus::Paused);
        match new {
            HotkeyStatus::Paused => self.toast(
                ToastCode::SecureInput,
                false,
                "Secure input is on; the shortcut is paused.".into(),
            ),
            HotkeyStatus::Unavailable { .. } | HotkeyStatus::NotRunning if working => self.toast(
                ToastCode::HotkeyFailed,
                true,
                "The global shortcut stopped working.".into(),
            ),
            _ => {}
        }
    }

    /// The mic button, the toggle deep link, `dictate_start_take`: a
    /// hands-free take for `origin`.
    pub fn start_take(&self, origin: Origin) -> Result<TakeId, DictateError> {
        if let Origin::Composer { .. } = origin {
            if self.inner.lock().mic_owner.is_some() {
                return Err(DictateError::MicBusy);
            }
        }
        self.act(Action::Arm, Some(origin));
        self.act(Action::Start { hands_free: true }, None);
        let id = self.current_take().ok_or(DictateError::NotStarted)?;
        self.inner.lock().gesture.hands_free();
        Ok(id)
    }

    /// "Use local for this take" (F-DI-05): transcribes the last failed cloud
    /// take on this device. False when there's nothing to retry, a newer take
    /// started, or no local model is loaded.
    pub fn use_local(&self) -> bool {
        if self.deps.engine.current().is_none() {
            self.toast(
                ToastCode::NoModel,
                true,
                "No local speech model is downloaded and loaded.".into(),
            );
            return false;
        }
        let (retry, id) = {
            let mut i = self.inner.lock();
            if i.machine.current().is_some() {
                return false;
            }
            let Some(retry) = i.retry.take() else {
                return false;
            };
            let (id, _) = i.machine.begin(retry.origin.clone());
            i.machine.advance(&id, TakeState::Transcribing);
            (retry, id)
        };
        let mut e = DictateStateEvent::new(DictatePhase::Transcribing, Some(id.clone()));
        if let Origin::Composer { target_id } = &retry.origin {
            e.composer = Some(target_id.clone());
        }
        self.emit(e);
        let cur = Current {
            id,
            target: retry.target,
            started: Instant::now(),
            shown: true,
            hands_free: false,
            warned: false,
        };
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("dictate-take".into())
            .spawn(move || {
                me.run_take(
                    cur,
                    retry.samples,
                    retry.end,
                    retry.duration,
                    retry.config,
                    false,
                )
            });
        true
    }

    /// One rewrite of `text` (F-DI-10): protected spellings go up wrapped and
    /// must come back unchanged, or the corrected un-rewritten text is kept.
    /// Over 5,000 characters nothing is sent; any failure keeps `text`. Both
    /// say so in a toast.
    fn rewrite(
        &self,
        mode: RewriteMode,
        text: &str,
        instructions: Option<&str>,
        rules: &[Rule],
        config: &DictateConfig,
        app: Option<&str>,
    ) -> String {
        let Some(rewriter) = self.deps.rewriter.clone() else {
            return text.to_string();
        };
        if text.chars().count() > REWRITE_MAX_CHARS {
            self.toast(
                ToastCode::TooLongToEnhance,
                false,
                format!(
                    "{} characters; the limit is {REWRITE_MAX_CHARS}.",
                    text.chars().count()
                ),
            );
            return text.to_string();
        }
        let terms = crate::dictionary::protected_terms(rules, config.language.as_deref(), app);
        let wrapped = crate::dictionary::wrap_protected(text, &terms);
        match rewriter.rewrite(mode, &wrapped, instructions, &terms) {
            Ok(out) => {
                let kept = crate::dictionary::accept_rewrite(text, &wrapped, &out);
                if kept.trim().is_empty() {
                    text.to_string()
                } else {
                    kept
                }
            }
            Err(f) => {
                self.deps.sink.toast(DictateToast {
                    code: match mode {
                        RewriteMode::Enhance => ToastCode::EnhanceFailed,
                        RewriteMode::Polish => ToastCode::PolishFailed,
                    },
                    error: false,
                    app: None,
                    model: None,
                    previous: None,
                    detail: f.detail,
                    message: None,
                    actions: Vec::new(),
                });
                text.to_string()
            }
        }
    }

    /// "Enhance" / "Polish" after a take (F-DI-10, UI-60): rewrites the last
    /// take typed into another app. If that window still has focus the
    /// result replaces the pasted text (undo, then paste); otherwise it's
    /// copied. False when there's no such take or one is running.
    pub fn quick_action(&self, mode: RewriteMode) -> bool {
        if self.deps.rewriter.is_none() {
            return false;
        }
        let last = {
            let i = self.inner.lock();
            if i.machine.current().is_some() {
                return false;
            }
            match i.last.clone() {
                Some(l) => l,
                None => return false,
            }
        };
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("dictate-rewrite".into())
            .spawn(move || me.run_quick_action(mode, last));
        true
    }

    fn run_quick_action(&self, mode: RewriteMode, last: LastTake) {
        self.emit(DictateStateEvent::new(phase_for(mode), None));
        let rules = self.deps.sink.rules();
        let instructions = last.config.instructions_for(last.app.as_deref());
        let out = self.rewrite(
            mode,
            &last.text,
            instructions.as_deref(),
            &rules,
            &last.config,
            last.app.as_deref(),
        );
        if out == last.text || self.inner.lock().machine.current().is_some() {
            // Unchanged (a failure, which toasted) or a new take started.
            self.emit(DictateStateEvent::new(DictatePhase::Idle, None));
            return;
        }
        let platform = self.deps.platform.as_ref();
        let same_window = || {
            let now = platform.capture_target().ok().flatten().map(|t| t.window);
            now.is_some() && now == last.target.as_ref().map(|t| t.window)
        };
        let terminal = last.target.as_ref().is_some_and(|t| t.terminal);
        let replaced = last.pasted
            && !terminal
            && last.config.output == Output::Paste
            && same_window()
            && platform.undo().is_ok();
        let outcome = if replaced {
            let (outcome, restore) =
                insert::insert(platform, last.target.as_ref(), &out, Output::Paste, &|| {
                    true
                });
            if let Some(r) = restore {
                self.schedule_restore(r);
            }
            outcome
        } else {
            match platform.write_clipboard(&out) {
                Ok(()) => Outcome::Copied(CopyReason::Chosen),
                Err(_) => Outcome::Dropped,
            }
        };
        match outcome {
            Outcome::Pasted => {
                let mut e = DictateStateEvent::new(DictatePhase::Inserted, None);
                e.words = Some(postproc::word_count(&out));
                self.emit(e);
            }
            Outcome::Copied(_) => {
                self.toast(
                    ToastCode::Copied,
                    false,
                    "The rewrite was copied: the field it was for no longer has focus.".into(),
                );
                self.emit(DictateStateEvent::new(DictatePhase::Copied, None));
            }
            Outcome::Dropped => self.emit(DictateStateEvent::new(DictatePhase::Idle, None)),
        }
        let mut i = self.inner.lock();
        if let Some(l) = &mut i.last {
            l.text = out;
            l.pasted = outcome == Outcome::Pasted;
        }
    }

    pub fn stop_take(&self) {
        self.inner.lock().gesture.reset();
        self.act(Action::Stop, None);
    }

    pub fn cancel(&self) {
        self.inner.lock().gesture.reset();
        self.act(Action::Cancel, None);
    }

    /// `tandem://dictate/toggle`.
    pub fn toggle(&self) {
        let recording = matches!(self.state(), TakeState::Arming | TakeState::Recording);
        if recording {
            self.stop_take();
        } else {
            let _ = self.start_take(Origin::Global);
        }
    }

    fn toast(&self, code: ToastCode, error: bool, detail: String) {
        self.deps.sink.toast(DictateToast {
            code,
            error,
            app: None,
            model: None,
            previous: None,
            detail,
            message: None,
            actions: Vec::new(),
        });
    }

    fn emit(&self, event: DictateStateEvent) {
        self.deps.sink.state(event);
    }

    fn act(&self, action: Action, origin: Option<Origin>) {
        match action {
            Action::Arm => self.arm(origin),
            Action::Start { hands_free } => self.show(hands_free),
            Action::Discard => self.discard(),
            Action::Cancel => self.cancel_now(),
            Action::Stop => self.finish(),
        }
    }

    fn arm(&self, origin: Option<Origin>) {
        let (enabled, restore, cloud) = {
            let mut i = self.inner.lock();
            (
                i.config.enabled,
                i.restore.take(),
                i.config.engine == EngineChoice::Cloud && self.deps.cloud.is_some(),
            )
        };
        if !enabled {
            return;
        }
        // A Conductor call holds the mic (F-CN-08): no take of any kind.
        if self.inner.lock().mic_owner.is_some() {
            self.toast(
                ToastCode::ConductorListening,
                true,
                "Conductor is listening".into(),
            );
            self.inner.lock().gesture.reset();
            return;
        }
        // A clipboard waiting to come back from the last take does now, so
        // this take saves the user's real clipboard.
        if let Some(r) = restore {
            insert::restore(self.deps.platform.as_ref(), &r);
        }
        // Cloud takes don't need a local model (F-DI-05).
        if !cloud && self.deps.engine.current().is_none() {
            self.toast(
                ToastCode::NoModel,
                true,
                "No local speech model is downloaded and loaded.".into(),
            );
            self.inner.lock().gesture.reset();
            return;
        }
        let origin = origin.unwrap_or_else(|| match self.deps.sink.composer_focus() {
            Some(target_id) if self.inner.lock().mic_owner.is_none() => {
                Origin::Composer { target_id }
            }
            _ => Origin::Global,
        });
        let (id, replaced) = {
            let mut i = self.inner.lock();
            // A new take: the last failed cloud take can't be retried now.
            i.retry = None;
            i.machine.begin(origin.clone())
        };
        if replaced.is_some() {
            self.deps.audio.cancel_take();
        }
        if let Err(err) = self.deps.audio.begin_take() {
            let (code, detail) = match &err {
                CaptureError::NoDevice => (ToastCode::NoMicrophone, err.to_string()),
                CaptureError::PermissionDenied => (ToastCode::MicPermission, err.to_string()),
                _ => (ToastCode::MicSilent, err.to_string()),
            };
            {
                let mut i = self.inner.lock();
                i.machine.fail(&id);
                i.gesture.reset();
            }
            self.toast(code, true, detail);
            self.emit(DictateStateEvent::new(DictatePhase::Error, Some(id)));
            return;
        }
        // The target is captured while the mic starts (composer takes have
        // none: their text goes straight to the caret).
        let target = match origin {
            Origin::Global => self.deps.platform.capture_target().unwrap_or_else(|err| {
                tracing::info!(error = %err, "dictate: no insertion target");
                None
            }),
            Origin::Composer { .. } => None,
        };
        let pause = self.inner.lock().config.pause_media;
        if pause {
            if let Some(m) = &self.deps.media {
                if let Err(err) = m.begin() {
                    tracing::info!(error = %err, "dictate: other audio not paused");
                }
            }
        }
        let _ = self.deps.engine.begin_take();
        self.inner.lock().current = Some(Current {
            id,
            target,
            started: Instant::now(),
            shown: false,
            hands_free: false,
            warned: false,
        });
    }

    fn show(&self, hands_free: bool) {
        let (event, sounds) = {
            let mut i = self.inner.lock();
            let Some(c) = i.current.as_mut() else { return };
            let id = c.id.clone();
            c.shown = true;
            c.hands_free = hands_free;
            c.started = Instant::now().min(c.started);
            if !i.machine.advance(&id, TakeState::Recording) {
                return;
            }
            let composer = match i.machine.origin(&id) {
                Some(Origin::Composer { target_id }) => Some(target_id.clone()),
                _ => None,
            };
            let mut e = DictateStateEvent::new(DictatePhase::Listening, Some(id));
            e.hands_free = hands_free;
            e.composer = composer;
            (e, i.config.sounds)
        };
        self.deps.hotkeys.listen_esc(true);
        if sounds {
            self.deps.chimes.play(Chime::Start);
        }
        self.emit(event);
    }

    /// An armed tap that never became a take: nothing shown, nothing kept.
    fn discard(&self) {
        let had = {
            let mut i = self.inner.lock();
            let had = i.current.take().is_some();
            i.machine.cancel(None);
            had
        };
        if had {
            self.deps.audio.cancel_take();
            self.after_take();
        }
    }

    /// Esc or ×: no transcription, nothing inserted, the clipboard untouched.
    fn cancel_now(&self) {
        let (had, id, shown) = {
            let mut i = self.inner.lock();
            let cur = i.current.take();
            i.machine.cancel(None);
            match cur {
                Some(c) => (true, Some(c.id), c.shown),
                None => (false, None, false),
            }
        };
        if !had {
            return;
        }
        self.deps.audio.cancel_take();
        self.deps.hotkeys.listen_esc(false);
        self.after_take();
        if shown {
            self.emit(DictateStateEvent::new(DictatePhase::Idle, id));
        }
    }

    fn after_take(&self) {
        if let Some(m) = &self.deps.media {
            if self.inner.lock().config.pause_media {
                m.end();
            }
        }
        if let Some(due) = self.deps.engine.end_take() {
            self.deps.sink.switch_due(due);
        }
    }

    fn finish(&self) {
        let Some(cur) = self.inner.lock().current.take() else {
            return;
        };
        let id = cur.id.clone();
        if !self
            .inner
            .lock()
            .machine
            .advance(&id, TakeState::Transcribing)
        {
            self.deps.audio.cancel_take();
            return;
        }
        let recording = self.deps.audio.end_take();
        self.deps.hotkeys.listen_esc(false);
        let (sounds, config) = {
            let i = self.inner.lock();
            (i.config.sounds, i.config.clone())
        };
        if sounds {
            self.deps.chimes.play(Chime::Stop);
        }
        self.emit(DictateStateEvent::new(
            DictatePhase::Transcribing,
            Some(id.clone()),
        ));
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("dictate-take".into())
            .spawn(move || me.transcribe_and_insert(cur, recording, config));
    }

    fn transcribe_and_insert(
        &self,
        cur: Current,
        recording: crate::capture::Recording,
        config: DictateConfig,
    ) {
        let samples = recording.samples;
        let duration = recording.duration;
        let id = cur.id.clone();
        let verdict = guards::check(&samples);
        let fail_guard = |code: ToastCode, detail: &str| {
            let mut i = self.inner.lock();
            if i.machine.fail(&id) {
                drop(i);
                self.toast(code, false, detail.into());
                self.emit(DictateStateEvent::new(
                    DictatePhase::Error,
                    Some(id.clone()),
                ));
            }
            self.after_take();
        };
        let end = match verdict {
            Verdict::Speech { end } => end,
            Verdict::NoSpeech => {
                return fail_guard(
                    ToastCode::NoSpeech,
                    "The whole take was quieter than -50 dBFS.",
                )
            }
            Verdict::TooShort => {
                return fail_guard(
                    ToastCode::TooShort,
                    "Less than 300 ms of the take was voiced.",
                )
            }
        };
        let use_cloud = config.engine == EngineChoice::Cloud && self.deps.cloud.is_some();
        self.run_take(cur, samples, end, duration, config, use_cloud);
    }

    /// Transcribes a voiced take (locally or in the cloud), cleans it up
    /// and inserts it. A cloud failure keeps the audio for `use_local`.
    fn run_take(
        &self,
        cur: Current,
        samples: Vec<f32>,
        end: usize,
        duration: Duration,
        config: DictateConfig,
        use_cloud: bool,
    ) {
        let id = cur.id.clone();
        let fail = |code: ToastCode, detail: String| {
            let mut i = self.inner.lock();
            if i.machine.fail(&id) {
                drop(i);
                self.toast(
                    code,
                    code != ToastCode::NoSpeech && code != ToastCode::TooShort,
                    detail,
                );
                self.emit(DictateStateEvent::new(
                    DictatePhase::Error,
                    Some(id.clone()),
                ));
            }
        };
        let rules = self.deps.sink.rules();
        let app = cur.target.as_ref().and_then(|t| t.app_id.clone());
        let dict = Dictionary::compile(&rules, config.language.as_deref(), app.as_deref());
        let voiced = guards::voiced_ratio(&samples[..end]);
        let (result, model) = if use_cloud {
            let Some(cloud) = self.deps.cloud.clone() else {
                return;
            };
            let hints = if config.share_hints {
                cloud_hints(&rules, config.language.as_deref(), app.as_deref())
            } else {
                Vec::new()
            };
            let got = cloud.transcribe(CloudRequest {
                pcm16k: &samples[..end],
                language: config.language.clone(),
                hints,
            });
            self.after_take();
            match got {
                Ok(text) => (Ok(text), "cloud".to_string()),
                Err(f) => {
                    // No silent fallback (F-DI-05): keep the audio and offer
                    // "Use local for this take".
                    let origin = self.inner.lock().machine.origin(&id).cloned();
                    let mut i = self.inner.lock();
                    if i.machine.fail(&id) {
                        if let Some(origin) = origin {
                            i.retry = Some(Retry {
                                origin,
                                target: cur.target.clone(),
                                samples,
                                end,
                                duration,
                                config: config.clone(),
                            });
                        }
                        drop(i);
                        self.deps.sink.toast(DictateToast {
                            code: ToastCode::CloudFailed,
                            error: true,
                            app: None,
                            model: None,
                            previous: None,
                            detail: f.detail,
                            message: Some(f.message),
                            actions: f.kind.actions(),
                        });
                        self.emit(DictateStateEvent::new(
                            DictatePhase::Error,
                            Some(id.clone()),
                        ));
                    }
                    return;
                }
            }
        } else {
            let Some(engine) = self.deps.engine.current() else {
                fail(
                    ToastCode::NoModel,
                    "No local speech model is loaded.".into(),
                );
                self.after_take();
                return;
            };
            let opts = SttOptions {
                language: config.language.clone(),
                initial_prompt: initial_prompt(&rules, config.language.as_deref(), app.as_deref()),
            };
            let result = engine.transcribe(&samples[..end], &opts);
            let model = engine.model_id().to_string();
            self.after_take();
            (result.map(|t| t.text).map_err(|e| e.to_string()), model)
        };
        let raw = match result {
            Ok(t) => t,
            Err(err) => {
                fail(ToastCode::TranscribeFailed, err);
                return;
            }
        };
        let Some(raw) = guards::filter_hallucination(&raw, voiced) else {
            fail(
                ToastCode::NoSpeech,
                "The model returned no words for this take.".into(),
            );
            return;
        };
        let text = postproc::apply(&raw, &dict);
        if text.trim().is_empty() {
            fail(
                ToastCode::NoSpeech,
                "Nothing was left after clean-up.".into(),
            );
            return;
        }
        // Auto-Enhance, or the formatting instructions as a Polish (F-DI-10).
        let instructions = config.instructions_for(app.as_deref());
        let mode = if config.auto_enhance {
            Some(RewriteMode::Enhance)
        } else {
            instructions.as_ref().map(|_| RewriteMode::Polish)
        };
        let text = match mode {
            Some(mode) => {
                let origin = self.inner.lock().machine.origin(&id).cloned();
                let mut e = DictateStateEvent::new(phase_for(mode), Some(id.clone()));
                if let Some(Origin::Composer { target_id }) = origin {
                    e.composer = Some(target_id);
                }
                self.emit(e);
                self.rewrite(
                    mode,
                    &text,
                    instructions.as_deref(),
                    &rules,
                    &config,
                    app.as_deref(),
                )
            }
            None => text,
        };
        {
            let mut i = self.inner.lock();
            if !i.machine.advance(&id, TakeState::PostProcessing)
                || !i.machine.advance(&id, TakeState::Inserting)
            {
                // A newer take started, or this one was cancelled.
                return;
            }
        }
        let origin = self.inner.lock().machine.origin(&id).cloned();
        let global = matches!(origin, Some(Origin::Global));
        let words = postproc::word_count(&text);
        let outcome = match origin {
            Some(Origin::Composer { target_id }) => {
                self.deps.sink.composer_insert(&target_id, &text);
                Outcome::Pasted
            }
            Some(Origin::Global) => {
                let machine = self.inner.clone();
                let check = id.clone();
                let still = move || machine.lock().machine.is_current(&check);
                let (outcome, restore) = insert::insert(
                    self.deps.platform.as_ref(),
                    cur.target.as_ref(),
                    &text,
                    config.output,
                    &still,
                );
                if let Some(r) = restore {
                    self.schedule_restore(r);
                }
                outcome
            }
            None => Outcome::Dropped,
        };
        let done = self.inner.lock().machine.advance(&id, TakeState::Done);
        if !done && outcome == Outcome::Dropped {
            return;
        }
        match &outcome {
            Outcome::Pasted => {
                let mut e = DictateStateEvent::new(DictatePhase::Inserted, Some(id.clone()));
                e.words = Some(words);
                self.emit(e);
            }
            Outcome::Copied(reason) => {
                let (code, app) = match reason {
                    CopyReason::Elevated { app } => (ToastCode::Elevated, Some(app.clone())),
                    _ => (ToastCode::Copied, None),
                };
                if *reason != CopyReason::Chosen {
                    self.deps.sink.toast(DictateToast {
                        code,
                        error: false,
                        app,
                        model: None,
                        previous: None,
                        detail: copy_detail(reason),
                        message: None,
                        actions: Vec::new(),
                    });
                }
                self.emit(DictateStateEvent::new(
                    DictatePhase::Copied,
                    Some(id.clone()),
                ));
            }
            Outcome::Dropped => {}
        }
        if global && outcome != Outcome::Dropped {
            self.inner.lock().last = Some(LastTake {
                text: text.clone(),
                target: cur.target.clone(),
                app: app.clone(),
                pasted: outcome == Outcome::Pasted,
                config: config.clone(),
            });
        }
        if outcome != Outcome::Dropped {
            self.deps.sink.finished(FinishedTake {
                id,
                text,
                model,
                word_count: words,
                duration_ms: duration.as_millis() as u64,
                app_id: app,
            });
        }
    }

    fn schedule_restore(&self, restore: PendingRestore) {
        self.inner.lock().restore = Some(restore.clone());
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("dictate-restore".into())
            .spawn(move || {
                std::thread::sleep(insert::RESTORE_AFTER);
                let pending = {
                    let mut i = me.inner.lock();
                    match &i.restore {
                        Some(r) if *r == restore => i.restore.take(),
                        _ => None,
                    }
                };
                if let Some(r) = pending {
                    insert::restore(me.deps.platform.as_ref(), &r);
                }
            });
    }
}

fn copy_detail(reason: &CopyReason) -> String {
    match reason {
        CopyReason::Chosen => "Where text goes is set to Copy to clipboard.".into(),
        CopyReason::NoTarget => {
            "The window the take started in couldn't be identified, so nothing was typed.".into()
        }
        CopyReason::Focus => {
            "Focus couldn't be given back to the window the take started in.".into()
        }
        CopyReason::Paste => "The paste keystroke couldn't be sent.".into(),
        CopyReason::Elevated { app } => {
            format!("{app} runs with administrator rights, which blocks input from other apps.")
        }
        CopyReason::Stale => "A newer take started before this one could be typed.".into(),
    }
}

/// Whisper's initial prompt: the preferred spellings, so names come out right.
fn initial_prompt(rules: &[Rule], lang: Option<&str>, app: Option<&str>) -> Option<String> {
    let mut words: Vec<String> = rules
        .iter()
        .filter(|r| r.kind == crate::dictionary::RuleKind::Word && r.in_scope(lang, app))
        .map(|r| r.replacement.clone())
        .collect();
    words.truncate(40);
    (!words.is_empty()).then(|| words.join(", "))
}

fn phase_for(mode: RewriteMode) -> DictatePhase {
    match mode {
        RewriteMode::Enhance => DictatePhase::Enhancing,
        RewriteMode::Polish => DictatePhase::Polishing,
    }
}

/// The preferred spellings sent with a cloud take when the user shares
/// dictionary hints (F-DI-05): the same words the local prompt gets.
fn cloud_hints(rules: &[Rule], lang: Option<&str>, app: Option<&str>) -> Vec<String> {
    let mut words: Vec<String> = rules
        .iter()
        .filter(|r| r.kind == crate::dictionary::RuleKind::Word && r.in_scope(lang, app))
        .map(|r| r.replacement.clone())
        .collect();
    words.truncate(40);
    words
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DictateError {
    #[error("Conductor is listening")]
    MicBusy,
    #[error("Dictate couldn't start")]
    NotStarted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureConfig, FakeAudioBackend};
    use crate::chimes::SilentPlayer;
    use crate::cloud::{CloudFailure, CloudFailureKind};
    use crate::hotkey::FakeHotkeys;
    use crate::insert::fake::FakePlatform;
    use crate::insert::Clip;
    use crate::stt::FakeStt;

    /// One rewrite the fake saw: mode, text, instructions, protected terms.
    type RewriteCall = (RewriteMode, String, Option<String>, Vec<String>);

    type Reply = Box<dyn Fn(&str) -> Result<String, CloudFailure> + Send + Sync>;

    /// A scripted rewriter: `reply` maps the text it gets to its answer.
    struct FakeRewriter {
        reply: Reply,
        calls: Mutex<Vec<RewriteCall>>,
    }

    impl FakeRewriter {
        fn new(
            reply: impl Fn(&str) -> Result<String, CloudFailure> + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                reply: Box::new(reply),
                calls: Mutex::new(Vec::new()),
            })
        }
    }

    impl Rewriter for FakeRewriter {
        fn rewrite(
            &self,
            mode: RewriteMode,
            text: &str,
            instructions: Option<&str>,
            protected: &[String],
        ) -> Result<String, CloudFailure> {
            self.calls.lock().push((
                mode,
                text.to_string(),
                instructions.map(str::to_string),
                protected.to_vec(),
            ));
            (self.reply)(text)
        }
    }

    /// One request the fake cloud saw: samples, language, hints.
    type CloudCall = (usize, Option<String>, Vec<String>);

    /// A scripted cloud: answers with `reply`, recording each request.
    struct FakeCloud {
        reply: Mutex<Result<String, CloudFailure>>,
        calls: Mutex<Vec<CloudCall>>,
    }

    impl FakeCloud {
        fn new(reply: Result<String, CloudFailure>) -> Arc<Self> {
            Arc::new(Self {
                reply: Mutex::new(reply),
                calls: Mutex::new(Vec::new()),
            })
        }
    }

    impl CloudStt for FakeCloud {
        fn transcribe(&self, req: CloudRequest<'_>) -> Result<String, CloudFailure> {
            self.calls
                .lock()
                .push((req.pcm16k.len(), req.language.clone(), req.hints.clone()));
            self.reply.lock().clone()
        }
    }

    #[derive(Default)]
    struct Sink {
        states: Mutex<Vec<DictateStateEvent>>,
        toasts: Mutex<Vec<DictateToast>>,
        inserted: Mutex<Vec<(String, String)>>,
        finished: Mutex<Vec<FinishedTake>>,
        rules: Mutex<Vec<Rule>>,
        focus: Mutex<Option<String>>,
        keys: Mutex<Vec<Phase>>,
        verified: Mutex<bool>,
        hotkey: Mutex<Vec<HotkeyStatus>>,
        fn_unheard: Mutex<u32>,
    }

    impl DictateSink for Sink {
        fn hotkey_status(&self, status: HotkeyStatus) {
            self.hotkey.lock().push(status);
        }
        fn state(&self, e: DictateStateEvent) {
            self.states.lock().push(e);
        }
        fn toast(&self, t: DictateToast) {
            self.toasts.lock().push(t);
        }
        fn composer_insert(&self, target: &str, text: &str) {
            self.inserted.lock().push((target.into(), text.into()));
        }
        fn finished(&self, t: FinishedTake) {
            self.finished.lock().push(t);
        }
        fn rules(&self) -> Vec<Rule> {
            self.rules.lock().clone()
        }
        fn composer_focus(&self) -> Option<String> {
            self.focus.lock().clone()
        }
        fn key(&self, phase: Phase, _native: bool) {
            self.keys.lock().push(phase);
        }
        fn verified(&self) {
            *self.verified.lock() = true;
        }
        fn switch_due(&self, _id: String) {}
        fn fn_unheard(&self) {
            *self.fn_unheard.lock() += 1;
        }
    }

    struct Rig {
        d: Dictate,
        sink: Arc<Sink>,
        keys: Arc<FakeHotkeys>,
        platform: Arc<FakePlatform>,
        stt: Arc<FakeStt>,
    }

    fn speech() -> Vec<f32> {
        (0..16_000 * 2)
            .map(|i| (i as f32 * 0.05).sin() * 0.3)
            .collect()
    }

    fn rig(samples: Vec<f32>, text: &str) -> Rig {
        rig_with(samples, text, None)
    }

    fn rig_with(samples: Vec<f32>, text: &str, cloud: Option<Arc<FakeCloud>>) -> Rig {
        rig_full(samples, text, cloud, None)
    }

    fn rig_full(
        samples: Vec<f32>,
        text: &str,
        cloud: Option<Arc<FakeCloud>>,
        rewriter: Option<Arc<FakeRewriter>>,
    ) -> Rig {
        let backend = Arc::new(FakeAudioBackend::with_samples(samples, 4));
        let audio = Arc::new(AudioService::start(
            backend,
            CaptureConfig::default(),
            Arc::new(|_| {}),
        ));
        let keys = Arc::new(FakeHotkeys::default());
        let platform = Arc::new(FakePlatform::default());
        platform.set_target(Some(Target {
            app_id: Some("org.gnome.gedit".into()),
            app_name: Some("Text Editor".into()),
            window: 7,
            element: 0,
            terminal: false,
            elevated: false,
            own: false,
        }));
        platform.set_clipboard(Clip::Text("previous".into()));
        let engine = Arc::new(EngineSlot::new());
        let stt = Arc::new(FakeStt::new("whisper-base.en", text));
        engine.install(stt.clone());
        let sink = Arc::new(Sink::default());
        let d = Dictate::new(
            DictateDeps {
                audio,
                hotkeys: keys.clone(),
                platform: platform.clone(),
                engine,
                chimes: Arc::new(SilentPlayer::default()),
                media: None,
                sink: sink.clone(),
                cloud: cloud.map(|c| c as Arc<dyn CloudStt>),
                rewriter: rewriter.map(|r| r as Arc<dyn Rewriter>),
            },
            DictateConfig::default(),
        );
        d.start_hotkeys(&Bindings {
            hold: Some("Ctrl+Alt".parse().expect("b")),
            toggle: None,
        });
        Rig {
            d,
            sink,
            keys,
            platform,
            stt,
        }
    }

    fn wait(cond: impl Fn() -> bool) {
        let t = Instant::now();
        while !cond() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn cloud_config(share_hints: bool) -> DictateConfig {
        DictateConfig {
            engine: EngineChoice::Cloud,
            share_hints,
            ..DictateConfig::default()
        }
    }

    fn hold_and_release(r: &Rig) {
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(600));
        r.keys.send(Which::Hold, Phase::Up);
    }

    fn enhance(auto: bool, formatting: Option<&str>, apps: &[(&str, &str)]) -> DictateConfig {
        DictateConfig {
            auto_enhance: auto,
            formatting: formatting.map(str::to_string),
            formatting_apps: apps
                .iter()
                .map(|(a, t)| (a.to_string(), t.to_string()))
                .collect(),
            ..DictateConfig::default()
        }
    }

    fn failure() -> CloudFailure {
        CloudFailure {
            kind: CloudFailureKind::Other,
            message: "x".into(),
            detail: "HTTP 503 UNAVAILABLE".into(),
        }
    }

    fn word_rule(word: &str) -> Rule {
        Rule {
            id: word.into(),
            kind: crate::dictionary::RuleKind::Word,
            replacement: word.into(),
            phrases: vec![],
            match_case: false,
            protect: true,
            spacing: crate::dictionary::Spacing::Exact,
            enabled: true,
            lang: "*".into(),
            app_profiles: vec![],
            updated_at: 0,
            deleted_at: None,
        }
    }

    #[test]
    fn f_di_10_auto_enhance_rewrites_every_take_before_it_is_inserted() {
        let rw = FakeRewriter::new(|t| Ok(format!("Goal: {t}")));
        let r = rig_full(speech(), "fix the login test", None, Some(rw.clone()));
        r.d.set_config(enhance(true, None, &[]));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(
            r.platform.pasted(),
            vec!["Goal: fix the login test".to_string()]
        );
        assert_eq!(rw.calls.lock()[0].0, RewriteMode::Enhance);
        let phases: Vec<DictatePhase> = r.sink.states.lock().iter().map(|e| e.state).collect();
        assert!(phases.contains(&DictatePhase::Enhancing), "{phases:?}");
    }

    #[test]
    fn f_di_10_formatting_instructions_polish_every_take_and_an_app_has_its_own() {
        let rw = FakeRewriter::new(|t| Ok(t.to_lowercase()));
        let r = rig_full(speech(), "Ship It", None, Some(rw.clone()));
        // The rig's target is gedit: its own instructions win.
        r.d.set_config(enhance(
            false,
            Some("Use full sentences."),
            &[("org.gnome.gedit", "Use lowercase.")],
        ));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["ship it".to_string()]);
        let c = rw.calls.lock()[0].clone();
        assert_eq!(
            (c.0, c.2.as_deref()),
            (RewriteMode::Polish, Some("Use lowercase."))
        );
        assert_eq!(
            enhance(false, Some("Use full sentences."), &[]).instructions_for(Some("slack")),
            Some("Use full sentences.".into())
        );
        assert_eq!(enhance(false, None, &[]).instructions_for(None), None);
    }

    #[test]
    fn f_di_10_1_over_5000_characters_is_refused_before_any_call() {
        let rw = FakeRewriter::new(|t| Ok(t.to_string()));
        let long = "word ".repeat(1_100);
        let r = rig_full(speech(), long.trim(), None, Some(rw.clone()));
        r.d.set_config(enhance(true, None, &[]));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert!(rw.calls.lock().is_empty(), "no call was made");
        assert!(r
            .sink
            .toasts
            .lock()
            .iter()
            .any(|t| t.code == ToastCode::TooLongToEnhance));
        assert_eq!(
            r.platform.pasted()[0].chars().count(),
            long.trim().chars().count()
        );
    }

    #[test]
    fn f_di_10_2_a_failed_rewrite_inserts_the_raw_transcript_and_says_so() {
        let rw = FakeRewriter::new(|_| Err(failure()));
        let r = rig_full(speech(), "hello world", None, Some(rw));
        r.d.set_config(enhance(true, None, &[]));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["hello world".to_string()]);
        let t = r
            .sink
            .toasts
            .lock()
            .iter()
            .find(|t| t.code == ToastCode::EnhanceFailed)
            .cloned();
        assert_eq!(t.map(|t| t.detail), Some("HTTP 503 UNAVAILABLE".into()));
    }

    #[test]
    fn f_di_10_2_a_failed_formatting_polish_says_polish_not_enhance() {
        let rw = FakeRewriter::new(|_| Err(failure()));
        let r = rig_full(speech(), "hello world", None, Some(rw));
        r.d.set_config(enhance(false, Some("Bullet points."), &[]));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["hello world".to_string()]);
        let codes: Vec<ToastCode> = r.sink.toasts.lock().iter().map(|t| t.code).collect();
        assert!(codes.contains(&ToastCode::PolishFailed), "{codes:?}");
        assert!(!codes.contains(&ToastCode::EnhanceFailed), "{codes:?}");
    }

    #[test]
    fn f_di_09_a_rewrite_that_changes_a_protected_spelling_is_not_used() {
        // The rewrite "corrects" Tauri to Tory: the un-rewritten text wins.
        let rw = FakeRewriter::new(|t| Ok(t.replace("⟦Tauri⟧", "⟦Tory⟧").replace("ship", "Ship")));
        let r = rig_full(speech(), "ship the Tauri build", None, Some(rw.clone()));
        *r.sink.rules.lock() = vec![word_rule("Tauri")];
        r.d.set_config(enhance(true, None, &[]));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(
            r.platform.pasted(),
            vec!["ship the Tauri build".to_string()]
        );
        let c = rw.calls.lock()[0].clone();
        assert_eq!(
            c.1, "ship the ⟦Tauri⟧ build",
            "protected terms go up wrapped"
        );
        assert_eq!(c.3, vec!["Tauri".to_string()]);
    }

    #[test]
    fn f_di_10_a_quick_action_replaces_the_pasted_text_while_its_field_has_focus() {
        let rw = FakeRewriter::new(|_| Ok("Hello, world.".into()));
        let r = rig_full(speech(), "hello world", None, Some(rw.clone()));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert!(rw.calls.lock().is_empty(), "no rewrite without asking");
        wait(|| r.platform.clipboard() == Clip::Text("previous".into()));
        assert!(r.d.quick_action(RewriteMode::Polish));
        wait(|| r.platform.pasted().first().map(String::as_str) == Some("Hello, world."));
        assert_eq!(r.platform.pasted(), vec!["Hello, world.".to_string()]);
        assert!(r
            .platform
            .calls()
            .contains(&crate::insert::fake::Call::Undo));
        assert_eq!(rw.calls.lock()[0].0, RewriteMode::Polish);
    }

    #[test]
    fn f_di_10_a_quick_action_copies_when_the_field_lost_focus() {
        let rw = FakeRewriter::new(|_| Ok("Goal: say hello.".into()));
        let r = rig_full(speech(), "hello world", None, Some(rw));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        wait(|| r.platform.clipboard() == Clip::Text("previous".into()));
        // The user moved to another window.
        r.platform.set_target(Some(Target {
            app_id: Some("org.mozilla.firefox".into()),
            app_name: Some("Firefox".into()),
            window: 99,
            element: 0,
            terminal: false,
            elevated: false,
            own: false,
        }));
        assert!(r.d.quick_action(RewriteMode::Enhance));
        wait(|| r.platform.clipboard() == Clip::Text("Goal: say hello.".into()));
        assert!(!r
            .platform
            .calls()
            .contains(&crate::insert::fake::Call::Undo));
        assert_eq!(
            r.platform.pasted(),
            vec!["hello world".to_string()],
            "nothing typed elsewhere"
        );
    }

    #[test]
    fn f_di_05_1_a_cloud_take_inserts_the_returned_text_and_never_runs_local() {
        let cloud = FakeCloud::new(Ok("cloud words".into()));
        let r = rig_with(speech(), "local words", Some(cloud.clone()));
        r.d.set_config(cloud_config(false));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["cloud words".to_string()]);
        assert_eq!(r.stt.calls(), 0, "local transcription never ran");
        let calls = cloud.calls.lock().clone();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0 > 0, "the voiced audio went up");
        assert!(calls[0].2.is_empty(), "no hints without the opt-in");
        wait(|| !r.sink.finished.lock().is_empty());
        assert_eq!(r.sink.finished.lock()[0].model, "cloud");
    }

    #[test]
    fn f_di_05_hints_go_up_only_with_the_opt_in() {
        let cloud = FakeCloud::new(Ok("use Tandem".into()));
        let r = rig_with(speech(), "x", Some(cloud.clone()));
        *r.sink.rules.lock() = vec![Rule {
            id: "r1".into(),
            kind: crate::dictionary::RuleKind::Word,
            replacement: "Tandem".into(),
            phrases: vec![],
            match_case: false,
            protect: true,
            spacing: crate::dictionary::Spacing::Exact,
            enabled: true,
            lang: "*".into(),
            app_profiles: vec![],
            updated_at: 0,
            deleted_at: None,
        }];
        r.d.set_config(cloud_config(true));
        hold_and_release(&r);
        wait(|| !cloud.calls.lock().is_empty());
        assert_eq!(cloud.calls.lock()[0].2, vec!["Tandem".to_string()]);
    }

    #[test]
    fn f_di_05_2_a_402_says_out_of_credits_and_inserts_nothing() {
        let cloud = FakeCloud::new(Err(CloudFailure {
            kind: CloudFailureKind::OutOfCredits,
            message: "Out of credits for cloud transcription".into(),
            detail: "402 CREDITS_EXHAUSTED".into(),
        }));
        let r = rig_with(speech(), "local words", Some(cloud.clone()));
        r.d.set_config(cloud_config(false));
        hold_and_release(&r);
        wait(|| !r.sink.toasts.lock().is_empty());
        std::thread::sleep(Duration::from_millis(300));
        let t = r.sink.toasts.lock()[0].clone();
        assert_eq!(t.code, ToastCode::CloudFailed);
        assert_eq!(
            t.message.as_deref(),
            Some("Out of credits for cloud transcription")
        );
        assert_eq!(t.actions, vec![ToastAction::UseLocal]);
        assert!(r.platform.pasted().is_empty(), "nothing inserted");
        assert_eq!(r.stt.calls(), 0, "no silent fallback to local");
        assert!(r.sink.finished.lock().is_empty());
    }

    #[test]
    fn f_di_05_3_use_local_runs_the_failed_take_on_this_device_only_when_asked() {
        let cloud = FakeCloud::new(Err(CloudFailure {
            kind: CloudFailureKind::Offline,
            message: "You're offline".into(),
            detail: "connection refused".into(),
        }));
        let r = rig_with(speech(), "local words", Some(cloud));
        r.d.set_config(cloud_config(false));
        hold_and_release(&r);
        wait(|| !r.sink.toasts.lock().is_empty());
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(r.stt.calls(), 0);
        assert!(r.d.use_local(), "the take is kept for the action");
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["local words".to_string()]);
        assert_eq!(r.stt.calls(), 1);
        assert!(!r.d.use_local(), "only once");
    }

    #[test]
    fn f_di_05_a_new_take_drops_the_retry() {
        let cloud = FakeCloud::new(Err(CloudFailure {
            kind: CloudFailureKind::Other,
            message: "Cloud transcription didn't work".into(),
            detail: "500".into(),
        }));
        let r = rig_with(speech(), "local words", Some(cloud));
        r.d.set_config(cloud_config(false));
        hold_and_release(&r);
        wait(|| !r.sink.toasts.lock().is_empty());
        std::thread::sleep(Duration::from_millis(200));
        // The user starts speaking again instead, then discards it.
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(300));
        r.d.cancel();
        r.keys.send(Which::Hold, Phase::Up);
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !r.d.use_local(),
            "the old take can't be inserted after a newer one"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(r.platform.pasted().is_empty());
    }

    #[test]
    fn f_di_05_cloud_takes_need_no_local_model() {
        let cloud = FakeCloud::new(Ok("from the cloud".into()));
        let r = rig_with(speech(), "x", Some(cloud));
        r.d.deps.engine.clear();
        r.d.set_config(cloud_config(false));
        hold_and_release(&r);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["from the cloud".to_string()]);
    }

    #[test]
    fn f_di_01_1_hold_speak_release_inserts_and_restores_the_clipboard() {
        let r = rig(speech(), "hello world");
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(600));
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["hello world".to_string()]);
        wait(|| r.platform.clipboard() == Clip::Text("previous".into()));
        assert_eq!(r.platform.clipboard(), Clip::Text("previous".into()));
        let states: Vec<DictatePhase> = r.sink.states.lock().iter().map(|e| e.state).collect();
        assert_eq!(
            states,
            vec![
                DictatePhase::Listening,
                DictatePhase::Transcribing,
                DictatePhase::Inserted
            ]
        );
        let f = r.sink.finished.lock();
        assert_eq!(f[0].word_count, 2);
        assert_eq!(f[0].app_id.as_deref(), Some("org.gnome.gedit"));
    }

    #[test]
    fn f_di_01_2_a_100ms_tap_records_nothing() {
        let r = rig(speech(), "hello");
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(100));
        r.keys.send(Which::Hold, Phase::Up);
        std::thread::sleep(Duration::from_millis(400));
        assert!(r.sink.states.lock().is_empty());
        assert_eq!(r.stt.calls(), 0);
        assert!(r.platform.pasted().is_empty());
        assert!(r.sink.finished.lock().is_empty());
    }

    #[test]
    fn f_di_01_3_esc_discards_without_transcribing() {
        let r = rig(speech(), "hello");
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(300));
        assert!(r.keys.esc_listening());
        r.keys.send(Which::Esc, Phase::Down);
        std::thread::sleep(Duration::from_millis(100));
        r.keys.send(Which::Hold, Phase::Up);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(r.stt.calls(), 0);
        assert!(r
            .platform
            .calls()
            .iter()
            .all(|c| matches!(c, crate::insert::fake::Call::CaptureTarget)));
        assert_eq!(r.platform.clipboard(), Clip::Text("previous".into()));
        assert_eq!(
            r.sink.states.lock().last().map(|e| e.state),
            Some(DictatePhase::Idle)
        );
    }

    #[test]
    fn f_di_01_3_esc_cancels_a_take_from_the_mic_button_or_the_link_too() {
        for origin in [
            Origin::Global,
            Origin::Composer {
                target_id: "thread:t1".into(),
            },
        ] {
            let r = rig(speech(), "hello");
            r.d.start_take(origin.clone()).expect("a take");
            std::thread::sleep(Duration::from_millis(300));
            assert!(r.keys.esc_listening());
            r.keys.send(Which::Esc, Phase::Down);
            wait(|| r.d.current_take().is_none());
            std::thread::sleep(Duration::from_millis(300));
            assert!(r.d.current_take().is_none(), "{origin:?}: Esc cancels");
            assert_eq!(r.stt.calls(), 0, "{origin:?}: nothing transcribed");
            assert!(r.platform.pasted().is_empty());
            assert!(r.sink.inserted.lock().is_empty());
        }
    }

    #[test]
    fn f_di_12_the_hold_key_during_a_mic_button_take_ends_it_without_losing_it() {
        let r = rig(speech(), "hello world");
        *r.sink.focus.lock() = Some("thread:t1".into());
        r.d.start_take(Origin::Composer {
            target_id: "thread:t1".into(),
        })
        .expect("a take");
        std::thread::sleep(Duration::from_millis(400));
        // A quick tap of the Dictate key stops the hands-free take, as it
        // does one started by double-tap; it doesn't start over.
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(60));
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| !r.sink.inserted.lock().is_empty());
        assert_eq!(r.stt.calls(), 1);
        assert!(!r.sink.inserted.lock().is_empty(), "the words were kept");
    }

    #[test]
    fn f_di_04_3_silence_shows_no_speech_and_inserts_nothing() {
        let r = rig(vec![0.0; 32_000], "Thank you.");
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(500));
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| !r.sink.toasts.lock().is_empty());
        assert_eq!(r.sink.toasts.lock()[0].code, ToastCode::NoSpeech);
        assert_eq!(r.stt.calls(), 0);
        assert!(r.platform.pasted().is_empty());
    }

    #[test]
    fn f_di_07_a_silent_mic_shows_reconnecting_then_listening_when_sound_is_back() {
        let r = rig(speech(), "hello");
        // No take: recovery events show nothing.
        r.d.on_capture_event(CaptureEvent::Rebuilt);
        assert!(r.sink.states.lock().is_empty());
        assert!(r.sink.toasts.lock().is_empty());
        r.d.start_take(Origin::Global).expect("a take");
        wait(|| {
            r.sink
                .states
                .lock()
                .last()
                .is_some_and(|e| e.state == DictatePhase::Listening)
        });
        let last = |r: &Rig| r.sink.states.lock().last().map(|e| (e.state, e.hands_free));
        r.d.on_capture_event(CaptureEvent::Rebuilt);
        assert_eq!(last(&r), Some((DictatePhase::Reconnecting, true)));
        assert_eq!(
            r.sink
                .toasts
                .lock()
                .iter()
                .map(|t| t.code)
                .collect::<Vec<_>>(),
            vec![ToastCode::Reconnecting]
        );
        r.d.on_capture_event(CaptureEvent::Resumed);
        assert_eq!(last(&r), Some((DictatePhase::Listening, true)));
        // The backup taking over also goes back to Listening.
        r.d.on_capture_event(CaptureEvent::Rebuilt);
        r.d.on_capture_event(CaptureEvent::SwitchedToBackup {
            device: "USB Mic".into(),
        });
        assert_eq!(last(&r), Some((DictatePhase::Listening, true)));
        r.d.cancel();
    }

    #[test]
    fn f_di_02_secure_input_pauses_dictate_and_says_so_then_it_comes_back() {
        let r = rig(speech(), "hello");
        r.keys.set_status(HotkeyStatus::Paused);
        wait(|| r.sink.hotkey.lock().contains(&HotkeyStatus::Paused));
        assert_eq!(r.sink.hotkey.lock().clone(), vec![HotkeyStatus::Paused]);
        let codes: Vec<ToastCode> = r.sink.toasts.lock().iter().map(|t| t.code).collect();
        assert_eq!(codes, vec![ToastCode::SecureInput]);
        r.keys.set_status(HotkeyStatus::Active);
        wait(|| r.sink.hotkey.lock().len() == 2);
        assert_eq!(r.sink.hotkey.lock()[1], HotkeyStatus::Active);
        // Coming back says nothing more.
        assert_eq!(r.sink.toasts.lock().len(), 1);
    }

    #[test]
    fn f_di_07_a_shortcut_that_stops_working_says_so_but_a_restart_does_not() {
        let r = rig(speech(), "hello");
        // The app restarting the shortcut (a new binding): no failure.
        r.d.stop_hotkeys();
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            r.sink.toasts.lock().is_empty(),
            "{:?}",
            r.sink.toasts.lock()
        );
        r.d.start_hotkeys(&Bindings {
            hold: Some("Ctrl+Alt".parse().expect("b")),
            toggle: None,
        });
        // The backend's session dies on its own (a Wayland portal closing).
        r.keys.set_status(HotkeyStatus::NotRunning);
        wait(|| !r.sink.toasts.lock().is_empty());
        let codes: Vec<ToastCode> = r.sink.toasts.lock().iter().map(|t| t.code).collect();
        assert_eq!(codes, vec![ToastCode::HotkeyFailed]);
        assert_eq!(r.sink.hotkey.lock().last(), Some(&HotkeyStatus::NotRunning));
    }

    #[test]
    fn f_di_07_a_restart_racing_the_status_poll_says_nothing() {
        let r = rig(speech(), "hello");
        // The app restarts the shortcut (a new binding) just as the clock
        // thread reads the backend's status, so what it reads is the gap
        // between stop and start: that's not the shortcut failing.
        let d = r.d.clone();
        r.keys.on_next_status(move || d.stop_hotkeys());
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            r.sink.toasts.lock().is_empty(),
            "{:?}",
            r.sink.toasts.lock()
        );
        assert!(
            r.sink.hotkey.lock().is_empty(),
            "{:?}",
            r.sink.hotkey.lock()
        );
    }

    #[test]
    fn f_di_04_the_ten_minute_cap_holds_when_the_shortcut_never_started() {
        use crate::hotkey::HotkeyBackend;
        let r = rig(speech(), "a long dictation");
        // The backend failed and dropped its sender (a Wayland portal
        // error, no display): only the deep link and the mic button remain.
        r.keys.stop();
        std::thread::sleep(Duration::from_millis(50));
        r.d.start_take(Origin::Global).expect("a take");
        wait(|| r.d.current_take().is_some());
        r.d.age_take(guards::WARN_AT + Duration::from_secs(1));
        wait(|| {
            r.sink
                .toasts
                .lock()
                .iter()
                .any(|t| t.code == ToastCode::TimeAlmostUp)
        });
        assert!(r
            .sink
            .toasts
            .lock()
            .iter()
            .any(|t| t.code == ToastCode::TimeAlmostUp));
        r.d.age_take(guards::MAX_TAKE - guards::WARN_AT);
        wait(|| r.d.current_take().is_none());
        assert!(r.d.current_take().is_none(), "the cap stops the take");
    }

    #[test]
    fn f_di_06_2_focus_failure_copies_and_toasts() {
        let r = rig(speech(), "hello there");
        r.platform.fail_focus(true);
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(500));
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| !r.sink.toasts.lock().is_empty());
        let t = r.sink.toasts.lock()[0].clone();
        assert_eq!(t.code, ToastCode::Copied);
        assert!(t.detail.contains("Focus"));
        assert_eq!(r.platform.clipboard(), Clip::Text("hello there".into()));
        assert!(r.platform.pasted().is_empty());
    }

    #[test]
    fn f_di_12_1_composer_takes_insert_directly_and_skip_the_clipboard() {
        let r = rig(speech(), "fix the test");
        *r.sink.focus.lock() = Some("thread:t1".into());
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(500));
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| !r.sink.inserted.lock().is_empty());
        assert_eq!(
            *r.sink.inserted.lock(),
            vec![("thread:t1".to_string(), "fix the test".to_string())]
        );
        assert!(r.platform.calls().is_empty(), "{:?}", r.platform.calls());
        assert_eq!(r.platform.clipboard(), Clip::Text("previous".into()));
        assert_eq!(
            r.sink.states.lock()[0].composer.as_deref(),
            Some("thread:t1")
        );
    }

    #[test]
    fn f_di_12_conductor_owning_the_mic_refuses_composer_takes() {
        let r = rig(speech(), "x");
        r.d.set_mic_owner(Some("conductor".into()));
        assert_eq!(
            r.d.start_take(Origin::Composer {
                target_id: "thread:t1".into()
            }),
            Err(DictateError::MicBusy)
        );
    }

    #[test]
    fn f_cn_08_a_call_holding_the_mic_refuses_the_global_key_too() {
        let r = rig(speech(), "x");
        r.d.set_mic_owner(Some("conductor".into()));
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(300));
        r.keys.send(Which::Hold, Phase::Up);
        std::thread::sleep(Duration::from_millis(200));
        assert!(r.d.current_take().is_none());
        assert!(r.platform.pasted().is_empty());
        r.d.set_mic_owner(None);
    }

    #[test]
    fn f_di_09_dictionary_applies_to_the_take() {
        let r = rig(speech(), "use tandem, https://tandem.dev");
        *r.sink.rules.lock() = vec![Rule::correction("r1", "tandem", "Tandem")];
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(500));
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(
            r.platform.pasted(),
            vec!["use Tandem, https://tandem.dev".to_string()]
        );
    }

    fn hold_fn(r: &Rig) {
        r.d.stop_hotkeys();
        r.d.start_hotkeys(&Bindings {
            hold: Some("Fn".parse().expect("fn")),
            toggle: None,
        });
    }

    #[test]
    fn f_di_02_fn_unheard_through_the_verify_test_is_reported_once() {
        let r = rig(speech(), "x");
        hold_fn(&r);
        r.d.verify_start();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(*r.sink.fn_unheard.lock(), 0, "not before the grace period");
        r.d.age_verify(FN_GRACE);
        wait(|| *r.sink.fn_unheard.lock() == 1);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(*r.sink.fn_unheard.lock(), 1);
    }

    #[test]
    fn f_di_02_a_heard_fn_another_hold_key_or_no_test_reports_nothing() {
        // Fn arrived: nothing to fall back from.
        let r = rig(speech(), "x");
        hold_fn(&r);
        r.d.verify_start();
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(50));
        r.d.age_verify(FN_GRACE);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(*r.sink.fn_unheard.lock(), 0);
        // Control+Option is the hold key: Fn doesn't matter.
        let r = rig(speech(), "x");
        r.d.verify_start();
        r.d.age_verify(FN_GRACE);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(*r.sink.fn_unheard.lock(), 0);
        // No verify test: an idle Fn key is just idle.
        let r = rig(speech(), "x");
        hold_fn(&r);
        r.d.verify_start();
        r.d.verify_stop();
        r.d.age_verify(FN_GRACE);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(*r.sink.fn_unheard.lock(), 0);
    }

    #[test]
    fn f_di_11_2_verification_needs_a_native_press_and_release() {
        let r = rig(speech(), "x");
        r.d.verify_start();
        r.keys.send(Which::Hold, Phase::Down);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!*r.sink.verified.lock());
        r.keys.send(Which::Hold, Phase::Up);
        wait(|| *r.sink.verified.lock());
        assert_eq!(*r.sink.keys.lock(), vec![Phase::Down, Phase::Up]);
        // The verify test never records.
        assert!(r.sink.states.lock().is_empty());
    }

    #[test]
    fn f_di_01_toggle_deep_link_starts_and_stops() {
        let r = rig(speech(), "toggled");
        r.d.toggle();
        assert_eq!(r.d.state(), TakeState::Recording);
        std::thread::sleep(Duration::from_millis(500));
        r.d.toggle();
        wait(|| !r.platform.pasted().is_empty());
        assert_eq!(r.platform.pasted(), vec!["toggled".to_string()]);
        assert!(r.sink.states.lock()[0].hands_free);
    }
}
