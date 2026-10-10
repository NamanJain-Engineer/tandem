//! Recording sounds (F-DI-03): soft start and stop chimes synthesised here,
//! so they play with the widget hidden and ship no audio files.

use std::f32::consts::TAU;

pub const RATE: u32 = 48_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chime {
    Start,
    Stop,
    Error,
}

/// A 120 ms sine sweep with a raised-cosine envelope at a soft level.
pub fn render(chime: Chime) -> Vec<f32> {
    let (from, to, ms) = match chime {
        Chime::Start => (660.0, 990.0, 120),
        Chime::Stop => (880.0, 587.0, 120),
        Chime::Error => (330.0, 247.0, 180),
    };
    sweep(from, to, ms, RATE, 0.18)
}

fn sweep(from: f32, to: f32, ms: u32, rate: u32, level: f32) -> Vec<f32> {
    let n = (rate * ms / 1000) as usize;
    let mut phase = 0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let freq = from + (to - from) * t;
            phase += TAU * freq / rate as f32;
            let env = 0.5 - 0.5 * (TAU * t).cos();
            phase.sin() * env * level
        })
        .collect()
}

/// Plays samples without blocking. Failures (no output device) are silent:
/// a chime is never worth an error.
pub trait ChimePlayer: Send + Sync {
    fn play(&self, chime: Chime);
}

/// Plays nothing (tests, the E2E harness), recording what it was asked.
#[derive(Default)]
pub struct SilentPlayer {
    pub played: parking_lot::Mutex<Vec<Chime>>,
}

impl ChimePlayer for SilentPlayer {
    fn play(&self, chime: Chime) {
        self.played.lock().push(chime);
    }
}

#[cfg(feature = "cpal")]
pub struct CpalPlayer;

#[cfg(feature = "cpal")]
impl ChimePlayer for CpalPlayer {
    fn play(&self, chime: Chime) {
        let samples = render(chime);
        let _ = std::thread::Builder::new()
            .name("dictate-chime".into())
            .spawn(move || {
                if let Err(err) = play_blocking(samples) {
                    tracing::debug!(error = %err, "dictate: chime not played");
                }
            });
    }
}

#[cfg(feature = "cpal")]
fn play_blocking(samples: Vec<f32>) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    // Resample by nearest index: a chime doesn't need better.
    let ratio = RATE as f32 / rate as f32;
    let len = (samples.len() as f32 / ratio) as usize;
    let mut pos = 0usize;
    let stream = device
        .build_output_stream(
            &config.into(),
            move |out: &mut [f32], _| {
                for frame in out.chunks_mut(channels) {
                    let s = if pos < len {
                        samples[((pos as f32) * ratio) as usize]
                    } else {
                        0.0
                    };
                    pos += 1;
                    frame.iter_mut().for_each(|x| *x = s);
                }
            },
            |e| tracing::debug!(error = %e, "dictate: chime stream error"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(
        (len as u64 * 1000 / rate as u64) + 80,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_03_chimes_are_short_soft_and_fade() {
        for c in [Chime::Start, Chime::Stop, Chime::Error] {
            let s = render(c);
            assert!(s.len() >= (RATE / 10) as usize);
            assert!(s.iter().all(|x| x.abs() <= 0.18 + 1e-6));
            assert!(
                s[0].abs() < 1e-3 && s[s.len() - 1].abs() < 1e-2,
                "fades in and out"
            );
        }
        assert_ne!(render(Chime::Start), render(Chime::Stop));
    }
}
