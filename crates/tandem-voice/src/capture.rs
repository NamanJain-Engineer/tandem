//! Microphone capture (F-DI-03, ARCHITECTURE §12.1).
//!
//! One input stream stays open, paused, while Dictate is enabled, so a take
//! starts by resuming it (target ≤50 ms). The backend's callback pushes mono
//! samples into an `rtrb` ring; a worker thread owns the stream, drains the
//! ring into the take while recording, watches for a silent stream (rebuild,
//! then the backup mic, then an error) and resamples to 16 kHz at the end.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::audio::{self, Gain};
use crate::devices::{self, DeviceInfo, MicPrefs};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("No microphone found")]
    NoDevice,
    #[error("Vibezeo can't use the microphone")]
    PermissionDenied,
    #[error("Your microphone isn't delivering audio")]
    NotDelivering,
    #[error("the microphone couldn't be opened: {0}")]
    Open(String),
    #[error("the microphone stopped: {0}")]
    Stream(String),
    #[error("capture isn't running")]
    Stopped,
}

/// Mono samples at the stream's own rate.
pub type SampleSink = Box<dyn FnMut(&[f32]) + Send>;
pub type ErrorSink = Box<dyn Fn(CaptureError) + Send>;

/// An open input stream. It may be tied to the thread that opened it (cpal
/// streams aren't `Send` everywhere), so only the capture worker touches it.
pub trait InputStream {
    fn play(&self) -> Result<(), CaptureError>;
    fn pause(&self) -> Result<(), CaptureError>;
    fn sample_rate(&self) -> u32;
}

pub trait AudioBackend: Send + Sync {
    fn devices(&self) -> Vec<DeviceInfo>;
    fn open(
        &self,
        device: &str,
        sink: SampleSink,
        on_error: ErrorSink,
    ) -> Result<Box<dyn InputStream>, CaptureError>;
}

/// What capture tells the voice service while a take runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEvent {
    /// The stream was silent and has been rebuilt on the same device.
    Rebuilt,
    /// "Microphone stopped responding — switched to backup".
    SwitchedToBackup { device: String },
    /// Recovery failed: "Your microphone isn't delivering audio".
    NotDelivering,
}

pub type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

/// A live stream's receiver (a Conductor call, F-CN-08): each block as it
/// arrives, mono at the stream's rate, after gain; and the stream's own
/// recovery events, which then don't reach Dictate.
/// Mono samples at the stream's rate.
pub type RateSink = Box<dyn FnMut(&[f32], u32) + Send>;

pub struct StreamTap {
    pub samples: RateSink,
    pub events: Box<dyn Fn(CaptureEvent) + Send>,
}

/// A finished take's audio.
#[derive(Debug, Clone, Default)]
pub struct Recording {
    /// 16 kHz mono.
    pub samples: Vec<f32>,
    pub device: Option<String>,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub prefs: MicPrefs,
    pub gain: Gain,
    /// How long a take may go without one non-zero sample before recovery
    /// steps in (1 s; tests shorten it).
    pub silence_timeout: Duration,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            prefs: MicPrefs::default(),
            gain: Gain::Off,
            silence_timeout: Duration::from_secs(1),
        }
    }
}

enum Cmd {
    Warm,
    Begin(mpsc::Sender<Result<(), CaptureError>>),
    BeginStream(StreamTap, mpsc::Sender<Result<(), CaptureError>>),
    End(mpsc::Sender<Recording>),
    Cancel,
    Config(CaptureConfig),
    Lost(u64),
    Reopen,
    Stop,
}

/// Two seconds of 48 kHz audio between the callback and the worker.
const RING: usize = 96_000;

pub struct AudioService {
    tx: mpsc::Sender<Cmd>,
    level: Arc<AtomicU32>,
    recording: Arc<AtomicBool>,
    backend: Arc<dyn AudioBackend>,
}

impl AudioService {
    pub fn start(backend: Arc<dyn AudioBackend>, config: CaptureConfig, events: EventSink) -> Self {
        let (tx, rx) = mpsc::channel();
        let level = Arc::new(AtomicU32::new(0));
        let recording = Arc::new(AtomicBool::new(false));
        let (b, t, l, r) = (
            backend.clone(),
            tx.clone(),
            level.clone(),
            recording.clone(),
        );
        // The stream lives and dies on this thread (it may not be `Send`).
        let _ = std::thread::Builder::new()
            .name("dictate-capture".into())
            .spawn(move || {
                Worker {
                    backend: b,
                    config,
                    events,
                    tx: t,
                    level: l,
                    recording: r,
                    stream: None,
                    generation: 0,
                    take: None,
                }
                .run(rx)
            });
        Self {
            tx,
            level,
            recording,
            backend,
        }
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.backend.devices()
    }

    /// Opens the stream ahead of the first take and leaves it paused.
    pub fn warm(&self) {
        let _ = self.tx.send(Cmd::Warm);
    }

    pub fn set_config(&self, config: CaptureConfig) {
        let _ = self.tx.send(Cmd::Config(config));
    }

    /// Resumes the stream into a new take buffer.
    pub fn begin_take(&self) -> Result<(), CaptureError> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Cmd::Begin(reply))
            .map_err(|_| CaptureError::Stopped)?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| CaptureError::Stopped)?
    }

    /// Resumes the stream and hands every block to `tap` until
    /// [`AudioService::end_take`] or [`AudioService::cancel_take`]. The same
    /// device policy, gain and silent-stream recovery as a take apply
    /// (F-CN-08); nothing is kept.
    pub fn begin_stream(&self, tap: StreamTap) -> Result<(), CaptureError> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Cmd::BeginStream(tap, reply))
            .map_err(|_| CaptureError::Stopped)?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| CaptureError::Stopped)?
    }

    /// Pauses the stream and returns the take's audio at 16 kHz.
    pub fn end_take(&self) -> Recording {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::End(reply)).is_err() {
            return Recording::default();
        }
        rx.recv_timeout(Duration::from_secs(30)).unwrap_or_default()
    }

    /// Closes the stream so the next take opens the device afresh (the
    /// device list or a fake source changed).
    pub fn reopen(&self) {
        let _ = self.tx.send(Cmd::Reopen);
    }

    /// Drops the take's audio unread.
    pub fn cancel_take(&self) {
        let _ = self.tx.send(Cmd::Cancel);
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Acquire)
    }

    /// The meter level (0..1) of the latest block while recording.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

impl Drop for AudioService {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

struct Open {
    stream: Box<dyn InputStream>,
    ring: rtrb::Consumer<f32>,
    device: String,
    rate: u32,
}

/// Recovery stages after a silent start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recovery {
    None,
    Rebuilt,
    Backup,
    Failed,
}

struct Take {
    buf: Vec<f32>,
    started: Instant,
    since: Instant,
    heard: bool,
    recovery: Recovery,
    tap: Option<StreamTap>,
}

struct Worker {
    backend: Arc<dyn AudioBackend>,
    config: CaptureConfig,
    events: EventSink,
    tx: mpsc::Sender<Cmd>,
    level: Arc<AtomicU32>,
    recording: Arc<AtomicBool>,
    stream: Option<Open>,
    generation: u64,
    take: Option<Take>,
}

impl Worker {
    fn run(mut self, rx: mpsc::Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok(Cmd::Stop) => break,
                Ok(cmd) => self.handle(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.drain();
            self.watch();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Warm => {
                if self.stream.is_none() {
                    if let Err(err) = self.open_preferred() {
                        tracing::info!(error = %err, "dictate: microphone not opened yet");
                    }
                }
            }
            Cmd::Begin(reply) => {
                let _ = reply.send(self.begin());
            }
            Cmd::BeginStream(tap, reply) => {
                let r = self.begin();
                if let Some(take) = &mut self.take {
                    take.tap = Some(tap);
                }
                let _ = reply.send(r);
            }
            Cmd::End(reply) => {
                let _ = reply.send(self.end());
            }
            Cmd::Cancel => {
                self.take = None;
                self.recording.store(false, Ordering::Release);
                self.pause();
            }
            Cmd::Config(config) => {
                let device_changed = config.prefs != self.config.prefs;
                self.config = config;
                // A new mic applies after the take ("Switches after this take").
                if device_changed && self.take.is_none() {
                    self.stream = None;
                }
            }
            Cmd::Reopen => {
                if self.take.is_none() {
                    self.stream = None;
                }
            }
            Cmd::Lost(generation) => {
                if generation == self.generation {
                    self.on_lost();
                }
            }
            Cmd::Stop => {}
        }
    }

    fn open_device(&mut self, device: &str) -> Result<(), CaptureError> {
        self.stream = None;
        self.generation += 1;
        let generation = self.generation;
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(RING);
        let sink: SampleSink = Box::new(move |samples: &[f32]| {
            for s in samples {
                if producer.push(*s).is_err() {
                    break;
                }
            }
        });
        let tx = self.tx.clone();
        let on_error: ErrorSink = Box::new(move |err| {
            tracing::warn!(error = %err, "dictate: input stream error");
            let _ = tx.send(Cmd::Lost(generation));
        });
        let stream = self.backend.open(device, sink, on_error)?;
        let rate = stream.sample_rate();
        self.stream = Some(Open {
            stream,
            ring: consumer,
            device: device.to_string(),
            rate,
        });
        Ok(())
    }

    fn open_preferred(&mut self) -> Result<(), CaptureError> {
        let devices = self.backend.devices();
        let id = devices::choose(&devices, &self.config.prefs).ok_or(CaptureError::NoDevice)?;
        self.open_device(&id)
    }

    fn begin(&mut self) -> Result<(), CaptureError> {
        if self.stream.is_none() {
            self.open_preferred()?;
        }
        if let Some(open) = &mut self.stream {
            // Whatever arrived while paused isn't this take's.
            while open.ring.pop().is_ok() {}
            if let Err(err) = open.stream.play() {
                // A stale stream (device unplugged while idle): reopen once.
                tracing::info!(error = %err, "dictate: reopening the microphone");
                self.open_preferred()?;
                if let Some(open) = &self.stream {
                    open.stream.play()?;
                }
            }
        }
        let now = Instant::now();
        self.take = Some(Take {
            buf: Vec::with_capacity(16_000 * 30),
            started: now,
            since: now,
            heard: false,
            recovery: Recovery::None,
            tap: None,
        });
        self.recording.store(true, Ordering::Release);
        Ok(())
    }

    fn pause(&mut self) {
        if let Some(open) = &self.stream {
            let _ = open.stream.pause();
        }
        self.level.store(0, Ordering::Relaxed);
    }

    fn end(&mut self) -> Recording {
        self.drain();
        self.recording.store(false, Ordering::Release);
        self.pause();
        let Some(take) = self.take.take() else {
            return Recording::default();
        };
        let rate = self.stream.as_ref().map(|o| o.rate).unwrap_or(16_000);
        let device = self.stream.as_ref().map(|o| o.device.clone());
        Recording {
            samples: audio::resample_to_16k(&take.buf, rate),
            device,
            duration: take.started.elapsed(),
        }
    }

    fn drain(&mut self) {
        let Some(open) = &mut self.stream else {
            return;
        };
        let n = open.ring.slots();
        if n == 0 {
            return;
        }
        let Some(take) = &mut self.take else {
            // Paused streams can still deliver a last block; drop it.
            while open.ring.pop().is_ok() {}
            return;
        };
        let start = take.buf.len();
        if let Ok(chunk) = open.ring.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            take.buf.extend_from_slice(a);
            take.buf.extend_from_slice(b);
            chunk.commit_all();
        }
        let block = &mut take.buf[start..];
        self.config.gain.apply(block);
        if !take.heard && block.iter().any(|s| *s != 0.0) {
            take.heard = true;
        }
        self.level
            .store(audio::level(block).to_bits(), Ordering::Relaxed);
        if let Some(tap) = &mut take.tap {
            (tap.samples)(block, open.rate);
            take.buf.truncate(start);
        }
    }

    /// Recovery events go to a stream's tap, or to Dictate.
    fn emit(&self, e: CaptureEvent) {
        match self.take.as_ref().and_then(|t| t.tap.as_ref()) {
            Some(tap) => (tap.events)(e),
            None => (self.events)(e),
        }
    }

    /// Silent-stream recovery: rebuild on the same device, then the backup,
    /// then give up with an error (F-DI-03 AC1).
    fn watch(&mut self) {
        let Some(take) = &self.take else {
            return;
        };
        if take.heard
            || take.recovery == Recovery::Failed
            || take.since.elapsed() < self.config.silence_timeout
        {
            return;
        }
        let stage = take.recovery;
        let current = self.stream.as_ref().map(|o| o.device.clone());
        match stage {
            Recovery::None => {
                let device = current.unwrap_or_default();
                let ok = self.open_device(&device).is_ok() && self.play();
                self.step(Recovery::Rebuilt);
                if ok {
                    self.emit(CaptureEvent::Rebuilt);
                } else {
                    self.watch_now();
                }
            }
            Recovery::Rebuilt => self.try_backup(current.as_deref()),
            Recovery::Backup | Recovery::Failed => {
                self.step(Recovery::Failed);
                self.emit(CaptureEvent::NotDelivering);
            }
        }
    }

    /// Moves straight to the next stage (the current one failed outright).
    fn watch_now(&mut self) {
        if let Some(take) = &mut self.take {
            take.since = Instant::now() - self.config.silence_timeout;
        }
    }

    fn try_backup(&mut self, current: Option<&str>) {
        let devices = self.backend.devices();
        let failed = current.unwrap_or_default().to_string();
        match devices::backup(&devices, &self.config.prefs, &failed) {
            Some(id) if self.open_device(&id).is_ok() && self.play() => {
                self.step(Recovery::Backup);
                self.emit(CaptureEvent::SwitchedToBackup { device: id });
            }
            _ => {
                self.step(Recovery::Failed);
                self.emit(CaptureEvent::NotDelivering);
            }
        }
    }

    fn play(&self) -> bool {
        self.stream
            .as_ref()
            .is_some_and(|o| o.stream.play().is_ok())
    }

    fn step(&mut self, to: Recovery) {
        if let Some(take) = &mut self.take {
            take.recovery = to;
            take.since = Instant::now();
        }
    }

    /// The device disappeared: idle, reopen on the next take; mid-take, the
    /// backup takes over at once.
    fn on_lost(&mut self) {
        let current = self.stream.as_ref().map(|o| o.device.clone());
        self.stream = None;
        if self.take.is_some() {
            self.try_backup(current.as_deref());
        }
    }
}

// ── Fakes ────────────────────────────────────────────────────────────────

/// What a fake device produces once playing.
#[derive(Debug, Clone)]
pub enum FakeSource {
    /// Zeros forever (a muted or broken mic).
    Zeros,
    /// These samples at this rate, in real time (`speed` × faster), then zeros.
    Samples {
        samples: Arc<Vec<f32>>,
        rate: u32,
        speed: u32,
    },
    /// Nothing at all.
    Nothing,
    /// Opening fails.
    Fails,
    /// Delivers like `Samples` until this many ms, then reports the device lost.
    DisconnectAfter { ms: u64 },
}

/// Fake devices for tests and the E2E harness (MILESTONES M8 "FakeDevice").
#[derive(Default)]
pub struct FakeAudioBackend {
    devices: Mutex<Vec<DeviceInfo>>,
    sources: Mutex<HashMap<String, FakeSource>>,
    opens: Mutex<Vec<String>>,
}

impl FakeAudioBackend {
    pub fn new(devices: Vec<(DeviceInfo, FakeSource)>) -> Self {
        let me = Self::default();
        for (d, s) in devices {
            me.sources.lock().insert(d.id.clone(), s);
            me.devices.lock().push(d);
        }
        me
    }

    /// One default mic playing `samples` (16 kHz) at `speed`× real time.
    pub fn with_samples(samples: Vec<f32>, speed: u32) -> Self {
        Self::new(vec![(
            DeviceInfo::new("fake-mic", "Fake Microphone", true),
            FakeSource::Samples {
                samples: Arc::new(samples),
                rate: 16_000,
                speed: speed.max(1),
            },
        )])
    }

    pub fn set_source(&self, id: &str, source: FakeSource) {
        self.sources.lock().insert(id.to_string(), source);
    }

    /// Every `open`, in order.
    pub fn opens(&self) -> Vec<String> {
        self.opens.lock().clone()
    }
}

impl AudioBackend for FakeAudioBackend {
    fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.lock().clone()
    }

    fn open(
        &self,
        device: &str,
        sink: SampleSink,
        on_error: ErrorSink,
    ) -> Result<Box<dyn InputStream>, CaptureError> {
        self.opens.lock().push(device.to_string());
        let source = self
            .sources
            .lock()
            .get(device)
            .cloned()
            .ok_or(CaptureError::NoDevice)?;
        if matches!(source, FakeSource::Fails) {
            return Err(CaptureError::Open("fake failure".into()));
        }
        Ok(Box::new(FakeStream::spawn(source, sink, on_error)))
    }
}

struct FakeStream {
    playing: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    rate: u32,
}

impl FakeStream {
    fn spawn(source: FakeSource, mut sink: SampleSink, on_error: ErrorSink) -> Self {
        let playing = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let rate = match &source {
            FakeSource::Samples { rate, .. } => *rate,
            _ => 16_000,
        };
        let (p, a) = (playing.clone(), alive.clone());
        let _ = std::thread::Builder::new()
            .name("fake-mic".into())
            .spawn(move || {
                let block = (rate / 100) as usize; // 10 ms
                let mut pos = 0usize;
                let mut played_ms = 0u64;
                let mut buf = vec![0f32; block];
                while a.load(Ordering::Acquire) {
                    let (speed, out): (u32, Option<&[f32]>) = match &source {
                        FakeSource::Nothing | FakeSource::Fails => (1, None),
                        FakeSource::Zeros | FakeSource::DisconnectAfter { .. } => {
                            (1, Some(&buf[..]))
                        }
                        FakeSource::Samples { samples, speed, .. } => {
                            buf.iter_mut().for_each(|s| *s = 0.0);
                            if pos < samples.len() {
                                let end = (pos + block).min(samples.len());
                                buf[..end - pos].copy_from_slice(&samples[pos..end]);
                            }
                            (*speed, Some(&buf[..]))
                        }
                    };
                    if p.load(Ordering::Acquire) {
                        if let FakeSource::DisconnectAfter { ms } = &source {
                            if played_ms >= *ms {
                                on_error(CaptureError::Stream("fake disconnect".into()));
                                a.store(false, Ordering::Release);
                                break;
                            }
                        }
                        if let Some(out) = out {
                            sink(out);
                            pos += block;
                        }
                        played_ms += 10;
                    }
                    std::thread::sleep(Duration::from_millis(10 / speed.max(1) as u64));
                }
            });
        Self {
            playing,
            alive,
            rate,
        }
    }
}

impl InputStream for FakeStream {
    fn play(&self) -> Result<(), CaptureError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(CaptureError::Stream("gone".into()));
        }
        self.playing.store(true, Ordering::Release);
        Ok(())
    }

    fn pause(&self) -> Result<(), CaptureError> {
        self.playing.store(false, Ordering::Release);
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect() -> (EventSink, Arc<Mutex<Vec<CaptureEvent>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        (Arc::new(move |e| s.lock().push(e)), seen)
    }

    fn quick(prefs: MicPrefs) -> CaptureConfig {
        CaptureConfig {
            prefs,
            gain: Gain::Off,
            silence_timeout: Duration::from_millis(80),
        }
    }

    fn wait_for(seen: &Arc<Mutex<Vec<CaptureEvent>>>, n: usize) {
        let t = Instant::now();
        while seen.lock().len() < n && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn f_di_03_records_and_resamples_a_take() {
        let tone: Vec<f32> = (0..16_000)
            .map(|i| ((i as f32) * 0.05).sin() * 0.3)
            .collect();
        let backend = Arc::new(FakeAudioBackend::with_samples(tone, 8));
        let (events, _) = collect();
        let svc = AudioService::start(backend, quick(MicPrefs::default()), events);
        svc.begin_take().expect("begin");
        assert!(svc.is_recording());
        std::thread::sleep(Duration::from_millis(250));
        let rec = svc.end_take();
        assert!(!svc.is_recording());
        assert!(rec.samples.len() > 8_000, "{}", rec.samples.len());
        assert!(rec.samples.iter().any(|s| s.abs() > 0.1));
        assert_eq!(rec.device.as_deref(), Some("fake-mic"));
    }

    #[test]
    fn f_di_03_begin_resumes_a_warm_stream_quickly() {
        let backend = Arc::new(FakeAudioBackend::with_samples(vec![0.1; 1600], 1));
        let (events, _) = collect();
        let svc = AudioService::start(backend.clone(), quick(MicPrefs::default()), events);
        svc.warm();
        std::thread::sleep(Duration::from_millis(30));
        let t = Instant::now();
        svc.begin_take().expect("begin");
        assert!(t.elapsed() < Duration::from_millis(50), "{:?}", t.elapsed());
        svc.cancel_take();
        assert_eq!(backend.opens(), vec!["fake-mic".to_string()]);
    }

    #[test]
    fn f_di_03_1_zeros_rebuild_then_fail_over_to_the_backup() {
        let backend = Arc::new(FakeAudioBackend::new(vec![
            (
                DeviceInfo::new("main", "Built-in Microphone", true),
                FakeSource::Zeros,
            ),
            (
                DeviceInfo::new("usb", "USB Microphone", false),
                FakeSource::Samples {
                    samples: Arc::new(vec![0.2; 16_000]),
                    rate: 16_000,
                    speed: 1,
                },
            ),
        ]));
        let (events, seen) = collect();
        let prefs = MicPrefs {
            primary: Some("main".into()),
            backup: Some("usb".into()),
        };
        let svc = AudioService::start(backend.clone(), quick(prefs), events);
        svc.begin_take().expect("begin");
        wait_for(&seen, 2);
        std::thread::sleep(Duration::from_millis(100));
        let rec = svc.end_take();
        assert_eq!(
            *seen.lock(),
            vec![
                CaptureEvent::Rebuilt,
                CaptureEvent::SwitchedToBackup {
                    device: "usb".into()
                }
            ]
        );
        assert_eq!(backend.opens(), vec!["main", "main", "usb"]);
        assert!(rec.samples.iter().any(|s| *s != 0.0));
    }

    #[test]
    fn f_di_03_1_no_backup_ends_in_the_error() {
        let backend = Arc::new(FakeAudioBackend::new(vec![(
            DeviceInfo::new("main", "Built-in Microphone", true),
            FakeSource::Zeros,
        )]));
        let (events, seen) = collect();
        let svc = AudioService::start(backend, quick(MicPrefs::default()), events);
        svc.begin_take().expect("begin");
        wait_for(&seen, 2);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            *seen.lock(),
            vec![CaptureEvent::Rebuilt, CaptureEvent::NotDelivering]
        );
        svc.cancel_take();
    }

    #[test]
    fn f_di_03_a_disconnect_mid_take_switches_to_the_backup() {
        let backend = Arc::new(FakeAudioBackend::new(vec![
            (
                DeviceInfo::new("main", "Built-in Microphone", true),
                FakeSource::DisconnectAfter { ms: 50 },
            ),
            (
                DeviceInfo::new("usb", "USB Microphone", false),
                FakeSource::Samples {
                    samples: Arc::new(vec![0.2; 16_000]),
                    rate: 16_000,
                    speed: 1,
                },
            ),
        ]));
        let (events, seen) = collect();
        let prefs = MicPrefs {
            primary: None,
            backup: Some("usb".into()),
        };
        let svc = AudioService::start(backend, quick(prefs), events);
        svc.begin_take().expect("begin");
        wait_for(&seen, 1);
        assert_eq!(
            seen.lock().first(),
            Some(&CaptureEvent::SwitchedToBackup {
                device: "usb".into()
            })
        );
        svc.cancel_take();
    }

    type Seen = (
        StreamTap,
        Arc<Mutex<(usize, u32)>>,
        Arc<Mutex<Vec<CaptureEvent>>>,
    );

    fn tap() -> Seen {
        let got = Arc::new(Mutex::new((0usize, 0u32)));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (g, e) = (got.clone(), events.clone());
        (
            StreamTap {
                samples: Box::new(move |s: &[f32], rate| {
                    let mut g = g.lock();
                    g.0 += s.len();
                    g.1 = rate;
                }),
                events: Box::new(move |ev| e.lock().push(ev)),
            },
            got,
            events,
        )
    }

    #[test]
    fn f_cn_08_1_a_call_never_opens_a_bluetooth_mic_on_its_own() {
        let tone = Arc::new(vec![0.25f32; 16_000]);
        let backend = Arc::new(FakeAudioBackend::new(vec![
            (
                DeviceInfo::new("bt", "AirPods Pro (Hands-Free)", true),
                FakeSource::Samples {
                    samples: tone.clone(),
                    rate: 16_000,
                    speed: 1,
                },
            ),
            (
                DeviceInfo::new("desk", "USB Desk Microphone", false),
                FakeSource::Samples {
                    samples: tone,
                    rate: 16_000,
                    speed: 1,
                },
            ),
        ]));
        let (dictate, dictate_events) = collect();
        let svc = AudioService::start(backend.clone(), quick(MicPrefs::default()), dictate);
        let (tap, got, _) = tap();
        svc.begin_stream(tap).expect("stream");
        let t = Instant::now();
        while got.lock().0 < 3_200 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(backend.opens(), vec!["desk".to_string()]);
        assert!(got.lock().0 >= 3_200, "blocks reach the call");
        assert_eq!(got.lock().1, 16_000);
        // Streamed audio isn't kept as a take.
        assert!(svc.end_take().samples.len() < 1_600);
        assert!(dictate_events.lock().is_empty());
    }

    #[test]
    fn f_cn_08_mic_gain_carries_across_calls() {
        let backend = Arc::new(FakeAudioBackend::new(vec![(
            DeviceInfo::new("desk", "USB Desk Microphone", false),
            FakeSource::Samples {
                samples: Arc::new(vec![0.25f32; 16_000]),
                rate: 16_000,
                speed: 1,
            },
        )]));
        let (dictate, _) = collect();
        let config = CaptureConfig {
            gain: Gain::X2,
            ..quick(MicPrefs::default())
        };
        let svc = AudioService::start(backend, config, dictate);
        // Each call streams, then lets go of the mic (`call_mic_stop`).
        let call = || {
            let peak = Arc::new(Mutex::new(0f32));
            let p = peak.clone();
            svc.begin_stream(StreamTap {
                samples: Box::new(move |s: &[f32], _| {
                    let mut p = p.lock();
                    *p = s.iter().fold(*p, |m, x| m.max(x.abs()));
                }),
                events: Box::new(|_| {}),
            })
            .expect("stream");
            let t = Instant::now();
            while *peak.lock() == 0.0 && t.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(5));
            }
            svc.cancel_take();
            let peak = *peak.lock();
            peak
        };
        // 2× through the soft clip.
        let mut want = [0.25f32];
        Gain::X2.apply(&mut want);
        assert!(want[0] > 0.4, "2× is louder");
        for n in 1..=3 {
            let peak = call();
            assert!(
                (peak - want[0]).abs() < 1e-6,
                "call {n} heard {peak}, not 2× 0.25"
            );
        }
    }

    #[test]
    fn f_cn_08_a_silent_mic_ends_the_take_on_the_call_not_in_dictate() {
        let backend = Arc::new(FakeAudioBackend::new(vec![(
            DeviceInfo::new("dead", "Dead Mic", true),
            FakeSource::Zeros,
        )]));
        let (dictate, dictate_events) = collect();
        let svc = AudioService::start(backend, quick(MicPrefs::default()), dictate);
        let (tap, _, events) = tap();
        svc.begin_stream(tap).expect("stream");
        wait_for(&events, 2);
        assert!(
            events.lock().contains(&CaptureEvent::NotDelivering),
            "{:?}",
            events.lock()
        );
        assert!(dictate_events.lock().is_empty());
        svc.cancel_take();
    }

    #[test]
    fn f_di_03_no_device_is_an_error() {
        let backend = Arc::new(FakeAudioBackend::default());
        let (events, _) = collect();
        let svc = AudioService::start(backend, quick(MicPrefs::default()), events);
        assert_eq!(svc.begin_take(), Err(CaptureError::NoDevice));
    }
}
