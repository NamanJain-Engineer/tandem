//! PCM16 at 24 kHz, the talker's audio format: a streaming resampler for the
//! mic (any rate in), conversions, base64 for the wire, and the level the
//! ring shows.

use base64::Engine as _;

use super::SAMPLE_RATE;

/// Linear resampling from the mic's rate to 24 kHz, block by block, with
/// no seam between blocks.
#[derive(Debug, Clone)]
pub struct Resampler {
    step: f64,
    /// Position of the next output sample, in input samples, relative to
    /// the start of the next block (may be negative: between the previous
    /// block's last sample and this block's first).
    pos: f64,
    last: f32,
}

impl Resampler {
    pub fn new(from_rate: u32) -> Self {
        Self {
            step: from_rate.max(1) as f64 / SAMPLE_RATE as f64,
            pos: 0.0,
            last: 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<i16>) {
        if input.is_empty() {
            return;
        }
        let n = input.len() as f64;
        while self.pos < n - 1.0 {
            let sample = if self.pos < 0.0 {
                let f = (self.pos + 1.0) as f32;
                self.last + (input[0] - self.last) * f
            } else {
                let j = self.pos as usize;
                let f = (self.pos - j as f64) as f32;
                input[j] + (input[j + 1] - input[j]) * f
            };
            out.push(to_i16(sample));
            self.pos += self.step;
        }
        self.pos -= n;
        self.last = input[input.len() - 1];
    }
}

pub fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

pub fn encode(samples: &[i16]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode(b64: &str) -> Option<Vec<i16>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}

/// The ring's 0..1 level for a block (peak, gently curved, as Dictate's).
pub fn level(samples: &[i16]) -> f32 {
    let peak = samples
        .iter()
        .map(|s| (*s as f32 / i16::MAX as f32).abs())
        .fold(0.0f32, f32::max);
    peak.sqrt().min(1.0)
}

/// Milliseconds of audio in `samples`.
pub fn millis(samples: usize) -> u64 {
    samples as u64 * 1000 / SAMPLE_RATE as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_duration_across_blocks() {
        let mut r = Resampler::new(48_000);
        let mut out = Vec::new();
        let block = vec![0.5f32; 480]; // 10 ms
        for _ in 0..100 {
            r.process(&block, &mut out);
        }
        // One second in, one second out (give or take a sample).
        assert!((out.len() as i64 - 24_000).abs() <= 2, "{}", out.len());
        assert!(out.iter().all(|s| (*s - to_i16(0.5)).abs() <= 1));
        let mut r = Resampler::new(16_000);
        let mut out = Vec::new();
        for _ in 0..100 {
            r.process(&vec![0.0f32; 160], &mut out);
        }
        assert!((out.len() as i64 - 24_000).abs() <= 2, "{}", out.len());
    }

    #[test]
    fn pcm_round_trips_through_base64() {
        let s = vec![0, 1, -1, i16::MAX, i16::MIN, 1234];
        assert_eq!(decode(&encode(&s)), Some(s));
        assert_eq!(decode("AAE"), None);
        assert_eq!(millis(24_000), 1000);
        assert!(level(&[0, 0]) == 0.0 && level(&[i16::MAX]) > 0.99);
    }
}
