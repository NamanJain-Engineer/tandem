//! The "Hey Conductor" wake phrase (F-CN-07, ARCHITECTURE §12.6).
//!
//! Detection runs on the device: a small sherpa-onnx keyword spotter bundled
//! under `models/wake/` (feature `kws`). The listener opens the Dictate
//! microphone through the DI-03 device policy, never claims the mic (it
//! steps aside whenever Dictate or a call holds it, see
//! [`crate::mic_owner`]) and pauses while media plays on a Bluetooth output
//! or, with "Pause while watching videos", while anything plays. The feature
//! ships disabled unless the false-accept benchmark recorded in
//! `models/wake/benchmark.json` passed.

pub mod bench;
pub mod listener;
pub mod process;
pub mod route;
#[cfg(feature = "kws")]
pub mod sherpa;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::mic_owner::MicOwner;

/// The keyword the bundled `keywords.txt` names.
pub const KEYWORD: &str = "HEY_CONDUCTOR";
/// The model files `models/wake/` must hold.
pub const MODEL_FILES: &[&str] = &[
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// "Sensitivity" (UI-67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Sensitivity {
    Low,
    #[default]
    Medium,
    High,
}

impl Sensitivity {
    pub fn parse(s: &str) -> Self {
        match s {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Medium,
        }
    }

    /// The keyword spotter's boosting score and trigger threshold. Medium is
    /// what the benchmark measured.
    pub fn tuning(self) -> (f32, f32) {
        match self {
            Self::Low => (1.5, 0.35),
            Self::Medium => (2.0, 0.25),
            Self::High => (2.5, 0.2),
        }
    }
}

/// A wake detector fed 16 kHz mono audio in small chunks.
pub trait WakeDetector: Send {
    /// Feeds samples; true when the phrase ended in them.
    fn accept(&mut self, samples: &[f32]) -> bool;
    /// Forgets buffered audio (after a pause, or once a call has started).
    fn reset(&mut self);
    /// False once a detector that runs elsewhere has gone; the listener
    /// loads a new one.
    fn alive(&self) -> bool {
        true
    }
}

/// The outcome of the false-accept benchmark, committed beside the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Benchmark {
    pub phrase: String,
    pub sensitivity: Sensitivity,
    pub true_clips: u32,
    pub true_triggers: u32,
    pub noise_minutes: u32,
    pub false_triggers: u32,
    /// Near-homophones, reported but not part of the bar.
    pub near_miss_clips: u32,
    pub near_miss_triggers: u32,
    pub passed: bool,
    pub method: String,
}

impl Benchmark {
    /// F-CN-07 AC1: at least 9 of 10 true utterances, and no trigger in 30
    /// minutes of speech and noise.
    pub fn meets_bar(&self) -> bool {
        self.true_clips >= 10
            && self.true_triggers * 10 >= self.true_clips * 9
            && self.noise_minutes >= 30
            && self.false_triggers == 0
    }
}

/// The benchmark as recorded when the model was bundled.
pub fn recorded_benchmark() -> Option<Benchmark> {
    serde_json::from_str(include_str!("../../models/wake/benchmark.json")).ok()
}

/// The model directory in the source tree (tests, dev builds). Packaged apps
/// pass their resource directory instead.
pub fn source_model_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("models/wake")
}

/// Why the wake phrase can't be turned on in this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WakeUnavailable {
    /// No keyword spotter in this build (the `tandem-wake` sidecar or the
    /// `kws` feature).
    NotBuilt,
    /// The bundled model files are missing.
    NoModel,
    /// The false-accept benchmark failed, so the feature ships disabled.
    BenchmarkFailed,
}

/// Whether this build can offer the wake phrase at all: `spotter` says
/// whether a keyword spotter exists (the sidecar, or `kws` in-process).
pub fn availability(model_dir: &Path, spotter: bool) -> Result<(), WakeUnavailable> {
    if !spotter {
        return Err(WakeUnavailable::NotBuilt);
    }
    if !recorded_benchmark().is_some_and(|b| b.passed && b.meets_bar()) {
        return Err(WakeUnavailable::BenchmarkFailed);
    }
    if !MODEL_FILES.iter().all(|f| model_dir.join(f).is_file()) {
        return Err(WakeUnavailable::NoModel);
    }
    Ok(())
}

/// Loads a detector for the bundled model: the `tandem-wake` sidecar when
/// `sidecar` is given, else in-process with the `kws` feature.
pub fn load_detector(
    sidecar: Option<&Path>,
    dir: &Path,
    sensitivity: Sensitivity,
) -> Result<Box<dyn WakeDetector>, String> {
    if let Some(bin) = sidecar {
        return process::ProcessDetector::spawn(bin, dir, sensitivity)
            .map(|d| Box::new(d) as Box<dyn WakeDetector>);
    }
    #[cfg(feature = "kws")]
    {
        sherpa::SherpaKws::load(dir, sensitivity).map(|d| Box::new(d) as Box<dyn WakeDetector>)
    }
    #[cfg(not(feature = "kws"))]
    {
        let _ = (dir, sensitivity);
        Err("built without the wake phrase".into())
    }
}

/// Why listening is paused right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PauseReason {
    /// Dictate is recording.
    Dictate,
    /// A Conductor call holds the mic.
    Call,
    /// Media is playing on a Bluetooth output (AirPods stay in A2DP).
    BluetoothMedia,
    /// "Pause while watching videos" and something is playing.
    Media,
}

/// What the listener is doing (`conductor:wake-state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
#[ts(export)]
pub enum WakeState {
    Off,
    Unavailable(WakeUnavailable),
    Paused(PauseReason),
    Listening,
    /// The mic couldn't be opened; retried on the next change.
    MicError,
}

/// Everything that decides whether the listener may hold the mic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GateInputs {
    pub enabled: bool,
    pub unavailable: Option<WakeUnavailable>,
    pub mic_owner: Option<MicOwner>,
    pub bluetooth_output_playing: bool,
    pub media_playing: bool,
    pub pause_while_videos: bool,
}

/// The pure pause rule (F-CN-07 behaviour).
pub fn gate(i: &GateInputs) -> WakeState {
    if !i.enabled {
        return WakeState::Off;
    }
    if let Some(u) = i.unavailable {
        return WakeState::Unavailable(u);
    }
    match i.mic_owner {
        Some(MicOwner::Dictate) => return WakeState::Paused(PauseReason::Dictate),
        Some(MicOwner::Call) => return WakeState::Paused(PauseReason::Call),
        None => {}
    }
    if i.bluetooth_output_playing {
        return WakeState::Paused(PauseReason::BluetoothMedia);
    }
    if i.pause_while_videos && i.media_playing {
        return WakeState::Paused(PauseReason::Media);
    }
    WakeState::Listening
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> GateInputs {
        GateInputs {
            enabled: true,
            pause_while_videos: true,
            ..Default::default()
        }
    }

    #[test]
    fn f_cn_07_pauses_while_dictate_or_a_call_owns_the_mic() {
        assert_eq!(gate(&on()), WakeState::Listening);
        let d = GateInputs {
            mic_owner: Some(MicOwner::Dictate),
            ..on()
        };
        assert_eq!(gate(&d), WakeState::Paused(PauseReason::Dictate));
        let c = GateInputs {
            mic_owner: Some(MicOwner::Call),
            ..on()
        };
        assert_eq!(gate(&c), WakeState::Paused(PauseReason::Call));
    }

    #[test]
    fn f_cn_07_pauses_for_bluetooth_media_and_videos() {
        let bt = GateInputs {
            bluetooth_output_playing: true,
            media_playing: true,
            pause_while_videos: false,
            ..on()
        };
        assert_eq!(gate(&bt), WakeState::Paused(PauseReason::BluetoothMedia));
        let video = GateInputs {
            media_playing: true,
            ..on()
        };
        assert_eq!(gate(&video), WakeState::Paused(PauseReason::Media));
        let ignore = GateInputs {
            media_playing: true,
            pause_while_videos: false,
            ..on()
        };
        assert_eq!(gate(&ignore), WakeState::Listening);
    }

    #[test]
    fn f_cn_07_off_and_unavailable_win() {
        assert_eq!(gate(&GateInputs::default()), WakeState::Off);
        let u = GateInputs {
            unavailable: Some(WakeUnavailable::BenchmarkFailed),
            ..on()
        };
        assert_eq!(
            gate(&u),
            WakeState::Unavailable(WakeUnavailable::BenchmarkFailed)
        );
    }

    #[test]
    fn f_cn_07_ships_disabled_unless_the_benchmark_passed() {
        let b = recorded_benchmark().expect("models/wake/benchmark.json parses");
        assert_eq!(
            b.passed,
            b.meets_bar(),
            "benchmark.json's `passed` must match its numbers"
        );
        let failing = Benchmark {
            false_triggers: 1,
            ..b.clone()
        };
        assert!(!failing.meets_bar());
        let weak = Benchmark {
            true_triggers: 8,
            ..b
        };
        assert!(!weak.meets_bar());
        assert_eq!(
            availability(&source_model_dir(), false),
            Err(WakeUnavailable::NotBuilt)
        );
        assert_eq!(availability(&source_model_dir(), true), Ok(()));
        let empty = tempfile::tempdir().expect("test");
        assert_eq!(
            availability(empty.path(), true),
            Err(WakeUnavailable::NoModel)
        );
    }

    #[test]
    fn f_cn_07_sensitivity_parses_with_medium_default() {
        assert_eq!(Sensitivity::parse("low"), Sensitivity::Low);
        assert_eq!(Sensitivity::parse("bogus"), Sensitivity::Medium);
        let (s_low, t_low) = Sensitivity::Low.tuning();
        let (s_high, t_high) = Sensitivity::High.tuning();
        assert!(s_low < s_high && t_low > t_high);
    }
}
