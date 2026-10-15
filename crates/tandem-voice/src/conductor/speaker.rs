//! The talker's voice out of the speakers (F-CN-08: the system's default
//! output). Audio arrives as PCM16 at 24 kHz and waits in a queue the output
//! stream drains; barge-in and hang-up empty it at once.

#[cfg(any(feature = "cpal", test))]
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::call::Speaker;
use crate::realtime::pcm;
#[cfg(any(feature = "cpal", test))]
use crate::realtime::SAMPLE_RATE;

/// Audio waiting to be played, shared with the output stream (cpal only;
/// builds without it, such as the macOS cross-check, play through nothing).
#[cfg(any(feature = "cpal", test))]
#[derive(Default)]
pub struct Queue {
    buf: Mutex<VecDeque<i16>>,
    level: AtomicU32,
    queued: AtomicU64,
    played: AtomicU64,
}

#[cfg(any(feature = "cpal", test))]
impl Queue {
    fn push(&self, samples: &[i16]) {
        self.level
            .store(pcm::level(samples).to_bits(), Ordering::Relaxed);
        let mut buf = self.buf.lock();
        buf.extend(samples);
        self.queued
            .fetch_add(samples.len() as u64, Ordering::Relaxed);
    }

    /// Drops what hasn't played; it no longer counts as queued.
    fn clear(&self) {
        let mut buf = self.buf.lock();
        self.queued.fetch_sub(buf.len() as u64, Ordering::Relaxed);
        buf.clear();
        self.level.store(0, Ordering::Relaxed);
    }

    fn pending(&self) -> usize {
        self.buf.lock().len()
    }

    fn level(&self) -> f32 {
        if self.pending() == 0 {
            0.0
        } else {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        }
    }

    /// The next sample, or silence.
    fn pop(&self) -> i16 {
        match self.buf.lock().pop_front() {
            Some(s) => {
                self.played.fetch_add(1, Ordering::Relaxed);
                s
            }
            None => 0,
        }
    }
}

/// Plays nothing, but keeps time as if it did (tests, the E2E harness, a
/// machine with no output device).
#[derive(Default)]
pub struct SilentSpeaker {
    until: Mutex<Option<Instant>>,
    level: AtomicU32,
    queued: AtomicU64,
}

impl SilentSpeaker {
    /// Samples still "playing" by the clock.
    fn remaining(&self, until: Option<Instant>) -> u64 {
        until
            .and_then(|u| u.checked_duration_since(Instant::now()))
            .map(|d| d.as_micros() as u64 * u64::from(crate::realtime::SAMPLE_RATE) / 1_000_000)
            .unwrap_or(0)
    }
}

impl Speaker for SilentSpeaker {
    fn play(&self, samples: &[i16]) {
        let len = Duration::from_millis(pcm::millis(samples.len()));
        let mut until = self.until.lock();
        let from = until
            .filter(|u| *u > Instant::now())
            .unwrap_or_else(Instant::now);
        *until = Some(from + len);
        self.queued
            .fetch_add(samples.len() as u64, Ordering::Relaxed);
        self.level
            .store(pcm::level(samples).to_bits(), Ordering::Relaxed);
    }

    fn stop(&self) {
        let mut until = self.until.lock();
        let left = self.remaining(*until);
        self.queued.fetch_sub(left, Ordering::Relaxed);
        *until = None;
    }

    fn speaking(&self) -> bool {
        self.until.lock().is_some_and(|u| u > Instant::now())
    }

    fn level(&self) -> f32 {
        if self.speaking() {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        } else {
            0.0
        }
    }

    fn queued(&self) -> u64 {
        self.queued.load(Ordering::Relaxed)
    }

    fn played(&self) -> u64 {
        let until = *self.until.lock();
        self.queued().saturating_sub(self.remaining(until))
    }
}

/// The default output device through cpal. The stream opens on the first
/// audio and stays open, playing silence when the queue is empty.
#[cfg(feature = "cpal")]
pub struct CpalSpeaker {
    queue: Arc<Queue>,
    opened: std::sync::Once,
}

#[cfg(feature = "cpal")]
impl Default for CpalSpeaker {
    fn default() -> Self {
        Self {
            queue: Arc::default(),
            opened: std::sync::Once::new(),
        }
    }
}

#[cfg(feature = "cpal")]
impl CpalSpeaker {
    fn open(&self) {
        let queue = self.queue.clone();
        self.opened.call_once(move || {
            let _ = std::thread::Builder::new()
                .name("conductor-speaker".into())
                .spawn(move || {
                    if let Err(err) = run_output(queue) {
                        tracing::info!(error = %err, "conductor: no audio output");
                    }
                });
        });
    }
}

#[cfg(feature = "cpal")]
fn run_output(queue: Arc<Queue>) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    // Each output frame advances through the 24 kHz queue by this much.
    let step = SAMPLE_RATE as f64 / rate.max(1) as f64;
    let mut acc = 0.0f64;
    let mut current = 0i16;
    let stream = device
        .build_output_stream(
            &config.into(),
            move |out: &mut [f32], _| {
                for frame in out.chunks_mut(channels) {
                    acc += step;
                    while acc >= 1.0 {
                        current = queue.pop();
                        acc -= 1.0;
                    }
                    let s = current as f32 / i16::MAX as f32;
                    frame.iter_mut().for_each(|x| *x = s);
                }
            },
            |e| tracing::debug!(error = %e, "conductor: output stream error"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    // The stream lives on this thread for the app's life.
    loop {
        std::thread::park();
    }
}

#[cfg(feature = "cpal")]
impl Speaker for CpalSpeaker {
    fn play(&self, samples: &[i16]) {
        self.open();
        self.queue.push(samples);
    }

    fn stop(&self) {
        self.queue.clear();
    }

    fn speaking(&self) -> bool {
        self.queue.pending() > 0
    }

    fn level(&self) -> f32 {
        self.queue.level()
    }

    fn queued(&self) -> u64 {
        self.queue.queued.load(Ordering::Relaxed)
    }

    fn played(&self) -> u64 {
        self.queue.played.load(Ordering::Relaxed)
    }
}

/// The speaker for this build: cpal when compiled in.
pub fn native() -> Arc<dyn Speaker> {
    #[cfg(feature = "cpal")]
    {
        Arc::new(CpalSpeaker::default())
    }
    #[cfg(not(feature = "cpal"))]
    {
        Arc::new(SilentSpeaker::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_01_the_silent_speaker_keeps_time_and_stops_at_once() {
        let s = SilentSpeaker::default();
        assert!(!s.speaking());
        s.play(&vec![1000; SAMPLE_RATE as usize]); // one second
        assert!(s.speaking());
        assert!(s.level() > 0.0);
        s.stop();
        assert!(!s.speaking());
        assert_eq!(s.level(), 0.0);
    }

    #[test]
    fn f_cn_01_the_queue_empties_on_barge_in() {
        let q = Queue::default();
        q.push(&[1, 2, 3]);
        assert_eq!((q.pop(), q.pending()), (1, 2));
        q.clear();
        assert_eq!((q.pop(), q.pending(), q.level()), (0, 0, 0.0));
    }

    #[test]
    fn f_cn_01_the_queue_counts_what_played_and_drops_the_rest_on_barge_in() {
        let q = Queue::default();
        q.push(&[1, 2, 3, 4, 5]);
        q.pop();
        q.pop();
        let count = |q: &Queue| {
            (
                q.queued.load(Ordering::Relaxed),
                q.played.load(Ordering::Relaxed),
            )
        };
        assert_eq!(count(&q), (5, 2));
        q.clear();
        // The three that never played aren't queued any more.
        assert_eq!(count(&q), (2, 2));
        // Silence doesn't count as played.
        q.pop();
        assert_eq!(count(&q), (2, 2));
    }

    #[test]
    fn f_cn_01_the_silent_speaker_plays_by_the_clock_and_stop_keeps_only_what_played() {
        let s = SilentSpeaker::default();
        s.play(&vec![1000; SAMPLE_RATE as usize]); // one second
        assert_eq!(s.queued(), u64::from(SAMPLE_RATE));
        std::thread::sleep(Duration::from_millis(200));
        let played = s.played();
        let ms = played * 1000 / u64::from(SAMPLE_RATE);
        assert!((150..600).contains(&ms), "{ms} ms");
        s.stop();
        assert_eq!(s.queued(), s.played());
        assert!(s.played() >= played);
    }
}
