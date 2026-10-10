//! What runs around speech-to-text so silence, a tap or a model's
//! hallucination never becomes text (F-DI-04 guards, ARCHITECTURE §12.4).

use std::time::Duration;

use crate::audio::{rms_dbfs, TARGET_RATE};

/// A take is at most ten minutes long.
pub const MAX_TAKE: Duration = Duration::from_secs(600);
/// The widget's timer turns amber with 30 s left.
pub const WARN_AT: Duration = Duration::from_secs(570);

/// Below this over the whole take, nothing was said.
const SILENCE_DBFS: f32 = -50.0;
/// A 20 ms window counts as voiced above this. Lower than the take gate, so
/// soft words between pauses survive ("quiet-speech window").
const VOICED_DBFS: f32 = -45.0;
/// Less voiced audio than this is "too short".
const MIN_VOICED: Duration = Duration::from_millis(300);
const WINDOW: usize = (TARGET_RATE / 50) as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Transcribe these samples (trailing silence trimmed).
    Speech { end: usize },
    /// "No speech detected".
    NoSpeech,
    /// "Too short — hold the key while you speak".
    TooShort,
}

/// Decides whether a 16 kHz take has speech worth transcribing.
pub fn check(samples: &[f32]) -> Verdict {
    if samples.is_empty() || rms_dbfs(samples) < SILENCE_DBFS {
        return Verdict::NoSpeech;
    }
    let voiced: Vec<bool> = samples
        .chunks(WINDOW)
        .map(|w| rms_dbfs(w) > VOICED_DBFS)
        .collect();
    let voiced_windows = voiced.iter().filter(|v| **v).count();
    if voiced_windows == 0 {
        return Verdict::NoSpeech;
    }
    let voiced_time = Duration::from_millis(voiced_windows as u64 * 20);
    if voiced_time < MIN_VOICED {
        return Verdict::TooShort;
    }
    Verdict::Speech {
        end: trim_end(samples, &voiced),
    }
}

/// Where the take ends once trailing silence is cut, keeping 200 ms of tail
/// so a soft final consonant isn't clipped.
fn trim_end(samples: &[f32], voiced: &[bool]) -> usize {
    let last = voiced.iter().rposition(|v| *v).unwrap_or(0);
    ((last + 1 + 10) * WINDOW).min(samples.len())
}

/// Voiced energy as a share of windows, for the hallucination filter.
pub fn voiced_ratio(samples: &[f32]) -> f32 {
    let windows: Vec<f32> = samples.chunks(WINDOW).map(rms_dbfs).collect();
    if windows.is_empty() {
        return 0.0;
    }
    windows.iter().filter(|d| **d > VOICED_DBFS).count() as f32 / windows.len() as f32
}

const HALLUCINATIONS: &[&str] = &[
    "thank you.",
    "thank you",
    "thanks for watching.",
    "thanks for watching!",
    "bye.",
    "bye",
    "you",
    "you.",
    "[blank_audio]",
    "[silence]",
    "(silence)",
    "[music]",
    "(music)",
    "[no speech]",
];

/// Drops what Whisper says over near-silence: a standalone "Thank you." or
/// "Bye." when little of the take was voiced, and markers like
/// `[BLANK_AUDIO]` always.
pub fn filter_hallucination(text: &str, voiced_ratio: f32) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_lowercase();
    let marker = lower.starts_with('[') && lower.ends_with(']')
        || lower.starts_with('(') && lower.ends_with(')');
    if marker {
        return None;
    }
    if voiced_ratio < 0.3 && HALLUCINATIONS.contains(&lower.as_str()) {
        return None;
    }
    // Whisper can still emit bracketed markers around real words.
    let cleaned = regex::Regex::new(r"\[(?i:blank_audio|silence|music|no speech)\]")
        .map(|re| re.replace_all(t, "").trim().to_string())
        .unwrap_or_else(|_| t.to_string());
    (!cleaned.is_empty()).then_some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(16_000.0 * secs) as usize)
            .map(|i| (i as f32 * 0.07).sin() * amp)
            .collect()
    }

    #[test]
    fn f_di_04_3_two_seconds_of_silence_is_no_speech() {
        assert_eq!(check(&vec![0.0; 32_000]), Verdict::NoSpeech);
        // Faint room noise too.
        let hiss: Vec<f32> = (0..32_000)
            .map(|i| ((i * 7919) % 13) as f32 * 1e-5)
            .collect();
        assert_eq!(check(&hiss), Verdict::NoSpeech);
    }

    #[test]
    fn f_di_04_too_short_and_trailing_trim() {
        let mut short = tone(0.1, 0.3);
        short.extend(vec![0.0; 16_000]);
        assert_eq!(check(&short), Verdict::TooShort);
        let mut speech = tone(1.0, 0.3);
        speech.extend(vec![0.0; 32_000]);
        match check(&speech) {
            Verdict::Speech { end } => assert!(end > 16_000 && end < 20_000, "{end}"),
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn f_di_04_soft_words_survive_the_gate() {
        // Quiet speech (about -40 dBFS) between pauses.
        let mut take = vec![0.0; 8_000];
        take.extend(tone(0.6, 0.015));
        take.extend(vec![0.0; 8_000]);
        assert!(
            matches!(check(&take), Verdict::Speech { .. }),
            "{:?}",
            check(&take)
        );
    }

    #[test]
    fn f_di_04_hallucinations_are_dropped_only_over_silence() {
        assert_eq!(filter_hallucination(" Thank you. ", 0.1), None);
        assert_eq!(
            filter_hallucination("Thank you.", 0.8).as_deref(),
            Some("Thank you.")
        );
        assert_eq!(filter_hallucination("[BLANK_AUDIO]", 0.9), None);
        assert_eq!(
            filter_hallucination("hello [BLANK_AUDIO] world", 0.9).as_deref(),
            Some("hello  world")
        );
        assert_eq!(filter_hallucination("   ", 0.9), None);
    }

    #[test]
    fn take_cap() {
        assert_eq!(MAX_TAKE - WARN_AT, Duration::from_secs(30));
    }
}
