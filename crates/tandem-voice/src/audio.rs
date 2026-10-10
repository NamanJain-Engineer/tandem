//! Sample arithmetic shared by capture, the guards and the tests: downmixing,
//! gain, resampling to 16 kHz, levels and WAV files (ARCHITECTURE §12.1).

use std::path::Path;

use rubato::{FftFixedIn, Resampler};

/// Whisper and Parakeet both take 16 kHz mono.
pub const TARGET_RATE: u32 = 16_000;

/// Interleaved frames to mono by averaging the channels.
pub fn downmix(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    out.extend(
        interleaved
            .chunks_exact(ch)
            .map(|frame| frame.iter().sum::<f32>() / ch as f32),
    );
}

/// Mic boost (F-DI-03): Off, 2×, 4×, 6×, 8×.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gain {
    Off,
    X2,
    X4,
    X6,
    X8,
}

impl Gain {
    /// The setting's value (`off`, `2x`, `4x`, `6x`, `8x`); anything else is Off.
    pub fn parse(s: &str) -> Self {
        match s {
            "2x" => Self::X2,
            "4x" => Self::X4,
            "6x" => Self::X6,
            "8x" => Self::X8,
            _ => Self::Off,
        }
    }

    pub fn factor(self) -> f32 {
        match self {
            Self::Off => 1.0,
            Self::X2 => 2.0,
            Self::X4 => 4.0,
            Self::X6 => 6.0,
            Self::X8 => 8.0,
        }
    }

    /// Multiplies with a soft clip (`tanh`), so a boosted shout saturates
    /// instead of wrapping. Off leaves samples untouched.
    pub fn apply(self, samples: &mut [f32]) {
        if self == Self::Off {
            return;
        }
        let k = self.factor();
        for s in samples {
            *s = (*s * k).tanh();
        }
    }
}

/// Resamples mono audio to 16 kHz. Rates already at 16 kHz are copied.
pub fn resample_to_16k(input: &[f32], rate: u32) -> Vec<f32> {
    if rate == TARGET_RATE || input.is_empty() || rate == 0 {
        return input.to_vec();
    }
    const CHUNK: usize = 1024;
    let Ok(mut rs) = FftFixedIn::<f32>::new(rate as usize, TARGET_RATE as usize, CHUNK, 2, 1)
    else {
        return linear_resample(input, rate);
    };
    let mut out = Vec::with_capacity(input.len() * TARGET_RATE as usize / rate as usize + CHUNK);
    let mut pos = 0;
    while pos + CHUNK <= input.len() {
        match rs.process(&[&input[pos..pos + CHUNK]], None) {
            Ok(chunk) => out.extend_from_slice(&chunk[0]),
            Err(_) => return linear_resample(input, rate),
        }
        pos += CHUNK;
    }
    // The tail, then the resampler's delay flushed out with silence.
    let tail = &input[pos..];
    if let Ok(chunk) = rs.process_partial(Some(&[tail]), None) {
        out.extend_from_slice(&chunk[0]);
    }
    if let Ok(chunk) = rs.process_partial::<&[f32]>(None, None) {
        out.extend_from_slice(&chunk[0]);
    }
    // FftFixedIn delays its output; drop the leading delay so timing lines up.
    let delay = rs.output_delay();
    let expected = (input.len() as u64 * TARGET_RATE as u64 / rate as u64) as usize;
    let start = delay.min(out.len());
    let mut out = out.split_off(start);
    out.truncate(expected);
    out
}

fn linear_resample(input: &[f32], rate: u32) -> Vec<f32> {
    let ratio = rate as f64 / TARGET_RATE as f64;
    let n = (input.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 * ratio;
            let j = x as usize;
            let f = (x - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// Root mean square in dBFS; silence is `-inf`.
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return f32::NEG_INFINITY;
    }
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        f32::NEG_INFINITY
    } else {
        (20.0 * rms.log10()) as f32
    }
}

/// The meter's 0..1 level for a block: peak, with a gentle curve.
pub fn level(samples: &[f32]) -> f32 {
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    peak.sqrt().min(1.0)
}

/// Reads a WAV file as mono f32 at its own rate.
pub fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), hound::Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mut mono = Vec::with_capacity(interleaved.len() / spec.channels.max(1) as usize);
    downmix(&interleaved, spec.channels, &mut mono);
    Ok((mono, spec.sample_rate))
}

/// Writes 16-bit mono PCM (fixtures, diagnostics in tests).
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    w.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| (i as f32 / rate as f32 * freq * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    #[test]
    fn f_di_03_downmix_averages_channels() {
        let mut out = Vec::new();
        downmix(&[1.0, 0.0, 0.5, 0.5], 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn f_di_03_gain_steps_soft_clip() {
        assert_eq!(Gain::parse("4x").factor(), 4.0);
        assert_eq!(Gain::parse("bogus"), Gain::Off);
        let mut s = [0.5f32, -0.9];
        Gain::X8.apply(&mut s);
        assert!(s[0] < 1.0 && s[0] > 0.99);
        assert!(s[1] > -1.0);
        let mut s = [0.25f32];
        Gain::Off.apply(&mut s);
        assert_eq!(s, [0.25]);
    }

    #[test]
    fn f_di_03_resamples_48k_to_16k_keeping_length_and_level() {
        let input = sine(440.0, 48_000, 1.0);
        let out = resample_to_16k(&input, 48_000);
        assert!((out.len() as i64 - 16_000).abs() < 50, "{}", out.len());
        let a = rms_dbfs(&input);
        let b = rms_dbfs(&out[200..15_000]);
        assert!((a - b).abs() < 1.0, "{a} vs {b}");
        assert_eq!(resample_to_16k(&input[..10], 16_000).len(), 10);
    }

    #[test]
    fn levels() {
        assert_eq!(rms_dbfs(&[0.0; 10]), f32::NEG_INFINITY);
        assert!((rms_dbfs(&[1.0; 10])).abs() < 0.01);
        assert!(level(&[0.25]) > 0.49);
    }

    #[test]
    fn wav_round_trip() {
        let dir = tempfile::tempdir().expect("dir");
        let p = dir.path().join("a.wav");
        let s = sine(300.0, 16_000, 0.1);
        write_wav(&p, &s, 16_000).expect("write");
        let (back, rate) = read_wav(&p).expect("read");
        assert_eq!(rate, 16_000);
        assert_eq!(back.len(), s.len());
        assert!((back[100] - s[100]).abs() < 0.001);
    }
}
