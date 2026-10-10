//! F-CN-07 AC1 against the bundled model: 9 of 10 true utterances trigger,
//! and 30 minutes of speech and noise trigger nothing. Synthetic fixtures
//! (scripts/gen-wake-fixtures.py); the benchmark on real hardware is manual
//! (HUMAN_TODO). Run with
//! `cargo test -p tandem-voice --features kws -- --include-ignored f_cn_07_1`.
#![cfg(feature = "kws")]

use std::path::PathBuf;

use tandem_voice::wake::bench::{parse_segments, speech_noise};
use tandem_voice::wake::sherpa::SherpaKws;
use tandem_voice::wake::{recorded_benchmark, source_model_dir, Sensitivity, WakeDetector};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wake")
}

fn read(path: &PathBuf) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).expect("fixture opens");
    let spec = r.spec();
    assert_eq!((spec.channels, spec.sample_rate), (1, 16_000), "{path:?}");
    match spec.bits_per_sample {
        8 => r
            .samples::<i8>()
            .map(|s| s.expect("test") as f32 / 128.0)
            .collect(),
        _ => r
            .samples::<i16>()
            .map(|s| s.expect("test") as f32 / 32768.0)
            .collect(),
    }
}

/// Feeds 100 ms chunks, as the listener does; counts triggers.
fn triggers(kws: &mut SherpaKws, samples: &[f32]) -> u32 {
    let mut n = 0;
    for chunk in samples.chunks(1_600) {
        n += kws.accept(chunk) as u32;
    }
    n += kws.accept(&vec![0.0; 16_000]) as u32;
    n
}

fn load() -> SherpaKws {
    SherpaKws::load(&source_model_dir(), Sensitivity::Medium).expect("bundled model loads")
}

fn true_triggers() -> u32 {
    let mut hits = 0;
    for i in 0..10 {
        let mut kws = load();
        let clip = read(&fixtures().join(format!("hey_conductor_{i:02}.wav")));
        let n = triggers(&mut kws, &clip);
        eprintln!("hey_conductor_{i:02}: {n}");
        hits += (n > 0) as u32;
    }
    hits
}

#[test]
fn f_cn_07_1_true_utterances_trigger() {
    let hits = true_triggers();
    assert!(hits >= 9, "{hits} of 10 true utterances triggered");
}

/// Near-homophones ("hey, conduct yourself", "hey Connor", "the
/// conductor") are measured on their own and recorded beside the benchmark;
/// they aren't part of AC1's bar.
fn near_miss_triggers() -> (u32, u32) {
    let audio = read(&fixtures().join("near_misses.wav"));
    let spans =
        parse_segments(&std::fs::read_to_string(fixtures().join("near_misses.txt")).expect("test"));
    let mut hits = 0;
    for s in &spans {
        let mut kws = load();
        hits += (triggers(&mut kws, &audio[s.start..s.end]) > 0) as u32;
    }
    (spans.len() as u32, hits)
}

#[test]
fn f_cn_07_1_everyday_speech_does_not_trigger() {
    let mut kws = load();
    let corpus = read(&fixtures().join("speech_corpus.wav"));
    assert_eq!(triggers(&mut kws, &corpus), 0);
}

#[test]
#[ignore = "30 minutes of audio through the model (about a minute); run with --include-ignored"]
fn f_cn_07_1_benchmark_thirty_minutes_of_speech_noise() {
    let corpus = read(&fixtures().join("speech_corpus.wav"));
    let spans = parse_segments(
        &std::fs::read_to_string(fixtures().join("speech_corpus.txt")).expect("test"),
    );
    let clip = speech_noise(&corpus, &spans, 30, 0x0C0D_0C70);
    assert_eq!(clip.len(), 30 * 60 * 16_000);
    let mut kws = load();
    let false_triggers = triggers(&mut kws, &clip);
    let true_hits = true_triggers();
    let (near_clips, near_hits) = near_miss_triggers();
    eprintln!("benchmark: {true_hits}/10 true, {false_triggers} false in 30 min, near misses {near_hits}/{near_clips}");
    let recorded = recorded_benchmark().expect("benchmark.json");
    assert_eq!(
        (recorded.true_triggers, recorded.false_triggers),
        (true_hits, false_triggers),
        "models/wake/benchmark.json must record this run"
    );
    assert_eq!(
        (recorded.near_miss_clips, recorded.near_miss_triggers),
        (near_clips, near_hits)
    );
    assert!(
        recorded.passed && recorded.meets_bar(),
        "the benchmark fails, so the wake phrase ships disabled"
    );
}

/// Writes the 30-minute clip to `$WAKE_DUMP` for tuning by hand.
#[test]
#[ignore = "tuning aid; set WAKE_DUMP=<path.wav>"]
fn dump_benchmark_clip() {
    let Ok(path) = std::env::var("WAKE_DUMP") else {
        return;
    };
    let corpus = read(&fixtures().join("speech_corpus.wav"));
    let spans = parse_segments(
        &std::fs::read_to_string(fixtures().join("speech_corpus.txt")).expect("test"),
    );
    let clip = speech_noise(&corpus, &spans, 30, 0x0C0D_0C70);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("test");
    for s in clip {
        w.write_sample((s * 32767.0) as i16).expect("test");
    }
    w.finalize().expect("test");
}

/// The app's path: the `tandem-wake` sidecar behind `ProcessDetector`.
#[test]
fn f_cn_07_1_sidecar_hears_the_phrase() {
    use tandem_voice::wake::process::ProcessDetector;
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_tandem-wake"));
    let mut d = ProcessDetector::spawn(bin, &source_model_dir(), Sensitivity::Medium)
        .expect("sidecar starts");
    let clip = read(&fixtures().join("hey_conductor_00.wav"));
    let mut heard = false;
    for chunk in clip
        .chunks(1_600)
        .chain(std::iter::once(&[0.0f32; 16_000][..]))
    {
        heard |= d.accept(chunk);
    }
    // Detections come back on a reader thread; give the last one a moment.
    for _ in 0..50 {
        if heard {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        heard |= d.accept(&[0.0; 160]);
    }
    assert!(heard, "the sidecar reported the phrase");
    assert!(d.alive());
    d.reset();
    for chunk in read(&fixtures().join("speech_corpus.wav")).chunks(1_600) {
        assert!(!d.accept(chunk), "everyday speech stays quiet");
    }
}
