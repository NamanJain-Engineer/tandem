//! The wake phrase's speech-and-noise benchmark (F-CN-07 AC1). A short
//! corpus of sentences that don't say the phrase (several of them close to
//! it: "the conductor", "hey Connor", "hey, conduct yourself") is stretched
//! into a long clip: sentences in a seeded random order with pauses between
//! them, a second, quieter talker over the top, and background noise whose
//! level drifts. The same seed always gives the same clip.

/// A sentence's span in the corpus, in samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Reads `segments.txt` lines (`start end text`); bad lines are skipped.
pub fn parse_segments(text: &str) -> Vec<Span> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let start = f.next()?.parse().ok()?;
            let end = f.next()?.parse().ok()?;
            (end > start).then_some(Span { start, end })
        })
        .collect()
}

/// A small, seeded xorshift generator (no `rand` dependency in this crate).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

const RATE: usize = 16_000;

/// Builds `minutes` of 16 kHz speech and noise from the corpus.
pub fn speech_noise(corpus: &[f32], spans: &[Span], minutes: u32, seed: u64) -> Vec<f32> {
    let len = minutes as usize * 60 * RATE;
    let mut out = vec![0.0f32; len];
    let mut rng = Rng::new(seed);
    let spans: Vec<Span> = spans
        .iter()
        .copied()
        .filter(|s| s.end <= corpus.len())
        .collect();
    if spans.is_empty() {
        return out;
    }
    // Two talkers: the main one at full level, a second one across the room.
    for (gain_lo, gain_hi, lead_in) in [(0.4f32, 1.0f32, 0usize), (0.08, 0.2, RATE * 3 / 2)] {
        let mut at = lead_in;
        while at < len {
            let s = spans[rng.below(spans.len())];
            let g = rng.range(gain_lo, gain_hi);
            for (i, x) in corpus[s.start..s.end].iter().enumerate() {
                if let Some(o) = out.get_mut(at + i) {
                    *o += x * g;
                }
            }
            // A pause of 0.3–2.5 s, and now and then a long quiet stretch.
            let pause = if rng.unit() < 0.05 {
                rng.range(5.0, 20.0)
            } else {
                rng.range(0.3, 2.5)
            };
            at += (s.end - s.start) + (pause * RATE as f32) as usize;
        }
    }
    // Pink-ish noise (white through a one-pole low-pass) whose level drifts
    // every 10 s between about -55 and -30 dBFS.
    let mut lp = 0.0f32;
    let block = 10 * RATE;
    for (b, chunk) in out.chunks_mut(block).enumerate() {
        let level = 10f32.powf(rng.range(-55.0, -30.0) / 20.0) * if b % 7 == 3 { 2.0 } else { 1.0 };
        for o in chunk.iter_mut() {
            let white = rng.unit() * 2.0 - 1.0;
            lp = 0.9 * lp + 0.1 * white;
            *o = (*o + lp * level * 6.0).clamp(-1.0, 1.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_07_speech_noise_is_seeded_and_full_length() {
        let corpus: Vec<f32> = (0..RATE * 4)
            .map(|i| (i as f32 * 0.05).sin() * 0.5)
            .collect();
        let spans = parse_segments("# start end text\n0 16000 one\n20000 48000 two\nbad line\n");
        assert_eq!(spans.len(), 2);
        let a = speech_noise(&corpus, &spans, 1, 7);
        let b = speech_noise(&corpus, &spans, 1, 7);
        assert_eq!(a.len(), 60 * RATE);
        assert_eq!(a, b);
        assert_ne!(a, speech_noise(&corpus, &spans, 1, 8));
        assert!(a.iter().all(|x| x.abs() <= 1.0));
        assert!(a.iter().any(|x| x.abs() > 0.3), "speech is in there");
    }
}
