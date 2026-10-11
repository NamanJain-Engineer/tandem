//! The wake listener: a worker thread that holds the Dictate microphone
//! (chosen by the DI-03 device policy) only while [`super::gate`] says
//! Listening, feeds the detector 16 kHz audio, and calls back when the
//! phrase is heard. It never claims the mic; when Dictate or a call takes
//! it, the listener closes its stream within one tick.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::route::OutputRoute;
use super::{gate, GateInputs, Sensitivity, WakeDetector, WakeState, WakeUnavailable};
use crate::capture::{AudioBackend, InputStream};
use crate::devices::{self, MicPrefs};
use crate::mic_owner::MicOwner;

/// Loads a detector for a sensitivity.
pub type DetectorFactory =
    Arc<dyn Fn(Sensitivity) -> Result<Box<dyn WakeDetector>, String> + Send + Sync>;
pub type WakeSink = Arc<dyn Fn() + Send + Sync>;
pub type StateSink = Arc<dyn Fn(WakeState) + Send + Sync>;
/// Who holds the mic now: the registry, plus anything that records
/// without claiming it.
pub type OwnerProbe = Arc<dyn Fn() -> Option<MicOwner> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WakeConfig {
    pub enabled: bool,
    pub sensitivity: Sensitivity,
    pub pause_while_videos: bool,
    /// The Dictate microphone setting (F-CN-08 shares it).
    pub prefs: MicPrefs,
}

pub struct ListenerDeps {
    pub backend: Arc<dyn AudioBackend>,
    pub mic_owner: OwnerProbe,
    pub route: Box<dyn OutputRoute>,
    pub detector: DetectorFactory,
    /// `None` when this build can offer the wake phrase.
    pub unavailable: Option<WakeUnavailable>,
    pub on_wake: WakeSink,
    pub on_state: StateSink,
}

/// How often the worker re-checks the gate.
const TICK: Duration = Duration::from_millis(50);
/// Output probes cost a process spawn; once a second is plenty.
const ROUTE_EVERY: Duration = Duration::from_secs(1);
/// After a trigger, ignore the tail of the same utterance.
const COOLDOWN: Duration = Duration::from_secs(2);
/// After a mic error, wait before opening again.
const RETRY: Duration = Duration::from_secs(5);

enum Cmd {
    Config(WakeConfig),
    Stop,
}

pub struct WakeListener {
    tx: mpsc::Sender<Cmd>,
    state: Arc<Mutex<WakeState>>,
}

impl WakeListener {
    pub fn start(deps: ListenerDeps, config: WakeConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(WakeState::Off));
        let s = state.clone();
        let spawned = std::thread::Builder::new()
            .name("wake-listener".into())
            .spawn(move || Worker::new(deps, config, s).run(rx));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "wake listener thread didn't start");
        }
        Self { tx, state }
    }

    pub fn configure(&self, config: WakeConfig) {
        let _ = self.tx.send(Cmd::Config(config));
    }

    pub fn state(&self) -> WakeState {
        *self.state.lock()
    }
}

impl Drop for WakeListener {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

struct Open {
    stream: Box<dyn InputStream>,
    buf: Arc<Mutex<Vec<f32>>>,
    failed: Arc<AtomicBool>,
    resampler: Linear,
}

struct Worker {
    deps: ListenerDeps,
    config: WakeConfig,
    state: Arc<Mutex<WakeState>>,
    detector: Option<(Sensitivity, Box<dyn WakeDetector>)>,
    open: Option<Open>,
    output: super::route::OutputState,
    route_at: Option<Instant>,
    cooldown_until: Option<Instant>,
    retry_at: Option<Instant>,
}

impl Worker {
    fn new(deps: ListenerDeps, config: WakeConfig, state: Arc<Mutex<WakeState>>) -> Self {
        Self {
            deps,
            config,
            state,
            detector: None,
            open: None,
            output: Default::default(),
            route_at: None,
            cooldown_until: None,
            retry_at: None,
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Cmd::Config(c)) => self.config = c,
                Ok(Cmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
        self.close();
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if self.config.enabled && self.route_at.is_none_or(|t| now >= t) {
            self.output = self.deps.route.state();
            self.route_at = Some(now + ROUTE_EVERY);
        }
        let mut next = gate(&GateInputs {
            enabled: self.config.enabled,
            unavailable: self.deps.unavailable,
            mic_owner: (self.deps.mic_owner)(),
            bluetooth_output_playing: self.output.bluetooth_playing,
            media_playing: self.output.playing,
            pause_while_videos: self.config.pause_while_videos,
        });
        if next == WakeState::Listening {
            if self.retry_at.is_some_and(|t| now < t) {
                next = WakeState::MicError;
            } else if let Err(e) = self.ensure_open() {
                tracing::warn!(error = %e, "wake listener can't listen");
                self.close();
                self.retry_at = Some(now + RETRY);
                next = WakeState::MicError;
            }
        }
        if next != WakeState::Listening {
            self.close();
        } else {
            self.listen(now);
        }
        let changed = {
            let mut s = self.state.lock();
            let changed = *s != next;
            *s = next;
            changed
        };
        if changed {
            (self.deps.on_state)(next);
        }
    }

    fn ensure_open(&mut self) -> Result<(), String> {
        let sens = self.config.sensitivity;
        if self
            .detector
            .as_ref()
            .is_none_or(|(s, d)| *s != sens || !d.alive())
        {
            self.detector = Some((sens, (self.deps.detector)(sens)?));
        }
        if self
            .open
            .as_ref()
            .is_some_and(|o| o.failed.load(Ordering::Relaxed))
        {
            self.open = None;
            return Err("the microphone stopped".into());
        }
        if self.open.is_some() {
            return Ok(());
        }
        let device = devices::choose(&self.deps.backend.devices(), &self.config.prefs)
            .ok_or("no microphone")?;
        let buf = Arc::new(Mutex::new(Vec::new()));
        let failed = Arc::new(AtomicBool::new(false));
        let (b, f) = (buf.clone(), failed.clone());
        let stream = self
            .deps
            .backend
            .open(
                &device,
                Box::new(move |s: &[f32]| {
                    let mut b = b.lock();
                    // Never more than 2 s waiting: an idle worker drops audio.
                    if b.len() < 96_000 {
                        b.extend_from_slice(s);
                    }
                }),
                Box::new(move |_| f.store(true, Ordering::Relaxed)),
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        let resampler = Linear::new(stream.sample_rate());
        self.open = Some(Open {
            stream,
            buf,
            failed,
            resampler,
        });
        if let Some((_, d)) = self.detector.as_mut() {
            d.reset();
        }
        Ok(())
    }

    fn listen(&mut self, now: Instant) {
        let (Some(open), Some((_, detector))) = (self.open.as_mut(), self.detector.as_mut()) else {
            return;
        };
        let raw = std::mem::take(&mut *open.buf.lock());
        if raw.is_empty() {
            return;
        }
        let samples = open.resampler.push(&raw);
        let heard = detector.accept(&samples);
        if heard && self.cooldown_until.is_none_or(|t| now >= t) {
            self.cooldown_until = Some(now + COOLDOWN);
            detector.reset();
            tracing::info!("wake phrase heard");
            (self.deps.on_wake)();
        }
    }

    fn close(&mut self) {
        if let Some(o) = self.open.take() {
            let _ = o.stream.pause();
            if let Some((_, d)) = self.detector.as_mut() {
                d.reset();
            }
        }
    }
}

/// A streaming linear resampler to 16 kHz, enough for keyword spotting.
struct Linear {
    step: f64,
    pos: f64,
    last: f32,
}

impl Linear {
    fn new(rate: u32) -> Self {
        Self {
            step: rate.max(1) as f64 / 16_000.0,
            pos: 0.0,
            last: 0.0,
        }
    }

    fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.step - 1.0).abs() < f64::EPSILON {
            return input.to_vec();
        }
        // `pos` is relative to `last` (index -1) .. input.
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        let at = |i: isize| if i < 0 { self.last } else { input[i as usize] };
        while self.pos < input.len() as f64 - 1.0 {
            let i = self.pos.floor() as isize;
            let frac = (self.pos - i as f64) as f32;
            out.push(at(i) * (1.0 - frac) + at(i + 1) * frac);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        if let Some(l) = input.last() {
            self.last = *l;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{FakeAudioBackend, FakeSource};
    use crate::devices::DeviceInfo;
    use crate::mic_owner::MicOwners;
    use crate::wake::route::{Fixed, OutputState};
    use crate::wake::PauseReason;
    use std::sync::atomic::AtomicUsize;

    /// Fires when it sees a sample above 0.9 (the fake "phrase").
    struct Spike;
    impl WakeDetector for Spike {
        fn accept(&mut self, s: &[f32]) -> bool {
            s.iter().any(|x| *x > 0.9)
        }
        fn reset(&mut self) {}
    }

    fn wait(mut f: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    struct Rig {
        listener: WakeListener,
        backend: Arc<FakeAudioBackend>,
        owners: MicOwners,
        wakes: Arc<AtomicUsize>,
    }

    fn rig(output: OutputState, devices: Vec<(DeviceInfo, FakeSource)>) -> Rig {
        let backend = Arc::new(FakeAudioBackend::new(devices));
        let owners = MicOwners::new();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let listener = WakeListener::start(
            ListenerDeps {
                backend: backend.clone(),
                mic_owner: {
                    let o = owners.clone();
                    Arc::new(move || o.current())
                },
                route: Box::new(Fixed(Mutex::new(output))),
                detector: Arc::new(|_| Ok(Box::new(Spike) as Box<dyn WakeDetector>)),
                unavailable: None,
                on_wake: Arc::new(move || {
                    w.fetch_add(1, Ordering::SeqCst);
                }),
                on_state: Arc::new(|_| {}),
            },
            WakeConfig {
                enabled: true,
                pause_while_videos: true,
                ..Default::default()
            },
        );
        Rig {
            listener,
            backend,
            owners,
            wakes,
        }
    }

    fn phrase() -> FakeSource {
        let mut s = vec![0.01f32; 16_000];
        s[8_000] = 0.95;
        FakeSource::Samples {
            samples: Arc::new(s),
            rate: 16_000,
            speed: 1,
        }
    }

    #[test]
    fn f_cn_07_hearing_the_phrase_calls_back_once() {
        let r = rig(
            OutputState::default(),
            vec![(DeviceInfo::new("mic", "USB Microphone", true), phrase())],
        );
        assert!(wait(|| r.wakes.load(Ordering::SeqCst) == 1));
        assert_eq!(r.listener.state(), WakeState::Listening);
    }

    #[test]
    fn f_cn_07_uses_the_di_03_policy_never_a_bluetooth_mic_on_its_own() {
        let r = rig(
            OutputState::default(),
            vec![
                (DeviceInfo::new("bluez_input.AA", "AirPods", true), phrase()),
                (
                    DeviceInfo::new("builtin", "Built-in Microphone", false),
                    FakeSource::Zeros,
                ),
            ],
        );
        assert!(wait(|| !r.backend.opens().is_empty()));
        assert_eq!(r.backend.opens()[0], "builtin");
    }

    #[test]
    fn f_cn_07_steps_aside_while_a_call_or_dictate_owns_the_mic() {
        let r = rig(
            OutputState::default(),
            vec![(
                DeviceInfo::new("mic", "USB Microphone", true),
                FakeSource::Zeros,
            )],
        );
        assert!(wait(|| r.listener.state() == WakeState::Listening));
        let call = r.owners.claim(MicOwner::Call).expect("test");
        assert!(wait(
            || r.listener.state() == WakeState::Paused(PauseReason::Call)
        ));
        drop(call);
        assert!(wait(|| r.listener.state() == WakeState::Listening));
        let _d = r.owners.claim(MicOwner::Dictate).expect("test");
        assert!(wait(
            || r.listener.state() == WakeState::Paused(PauseReason::Dictate)
        ));
        assert_eq!(r.wakes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn f_cn_07_pauses_for_bluetooth_media() {
        let r = rig(
            OutputState {
                playing: true,
                bluetooth_playing: true,
            },
            vec![(DeviceInfo::new("mic", "USB Microphone", true), phrase())],
        );
        assert!(wait(
            || r.listener.state() == WakeState::Paused(PauseReason::BluetoothMedia)
        ));
        std::thread::sleep(Duration::from_millis(300));
        assert!(r.backend.opens().is_empty(), "the mic stays closed");
        assert_eq!(r.wakes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn f_cn_07_off_by_default_opens_nothing() {
        let r = rig(
            OutputState::default(),
            vec![(DeviceInfo::new("mic", "Mic", true), phrase())],
        );
        r.listener.configure(WakeConfig::default());
        let backend = r.backend.clone();
        assert!(wait(|| r.listener.state() == WakeState::Off));
        let before = backend.opens().len();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(backend.opens().len(), before);
    }

    #[test]
    fn linear_resampler_keeps_duration() {
        let mut l = Linear::new(48_000);
        let mut n = 0;
        for _ in 0..10 {
            n += l.push(&vec![0.5; 4_800]).len();
        }
        assert!((15_990..=16_010).contains(&n), "{n}");
    }
}
