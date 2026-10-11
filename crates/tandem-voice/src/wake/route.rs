//! What the audio output is doing, for the wake listener's pause rules
//! (F-CN-07): media on a Bluetooth output pauses listening (so AirPods stay
//! in their media profile instead of dropping to a headset profile), and
//! "Pause while watching videos" pauses for anything playing.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutputState {
    /// Some app is playing audio.
    pub playing: bool,
    /// It plays on a Bluetooth output.
    pub bluetooth_playing: bool,
}

pub trait OutputRoute: Send + Sync {
    fn state(&self) -> OutputState;
}

/// The platform's probe. Linux asks PulseAudio / PipeWire through `pactl`;
/// macOS and Windows have none yet and report nothing playing (BUILD_LOG
/// D-1206).
pub fn native() -> Box<dyn OutputRoute> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::Pactl)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Box::new(Silent)
    }
}

/// Reports nothing playing.
pub struct Silent;

impl OutputRoute for Silent {
    fn state(&self) -> OutputState {
        OutputState::default()
    }
}

/// A fixed answer, for tests and the E2E harness.
pub struct Fixed(pub parking_lot::Mutex<OutputState>);

impl OutputRoute for Fixed {
    fn state(&self) -> OutputState {
        *self.0.lock()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::process::Command;

    pub struct Pactl;

    impl OutputRoute for Pactl {
        fn state(&self) -> OutputState {
            let run = |args: &[&str]| {
                Command::new("pactl")
                    .args(args)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default()
            };
            parse_pactl(
                &run(&["list", "short", "sinks"]),
                &run(&["list", "short", "sink-inputs"]),
            )
        }
    }
}

/// `pactl list short sinks` (id, name, driver, spec, state) and
/// `pactl list short sink-inputs` (id, sink id, client, driver, spec): a
/// stream is playing when it's attached to a RUNNING sink.
pub fn parse_pactl(sinks: &str, inputs: &str) -> OutputState {
    let running: Vec<(&str, bool)> = sinks
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 5 && f[4].trim() == "RUNNING").then(|| {
                (
                    f[0].trim(),
                    f[1].contains("bluez") || f[1].contains("bluetooth"),
                )
            })
        })
        .collect();
    let mut out = OutputState::default();
    for l in inputs.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        let Some(sink) = f.get(1).map(|s| s.trim()) else {
            continue;
        };
        if let Some((_, bt)) = running.iter().find(|(id, _)| *id == sink) {
            out.playing = true;
            out.bluetooth_playing |= *bt;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINKS: &str = "47\talsa_output.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n\
                         81\tbluez_output.AA_BB_CC_DD_EE_FF.1\tPipeWire\ts16le 2ch 48000Hz\tRUNNING\n";

    #[test]
    fn f_cn_07_bluetooth_media_is_detected() {
        let s = parse_pactl(SINKS, "12\t81\t90\tPipeWire\tfloat32le 2ch 48000Hz\n");
        assert_eq!(
            s,
            OutputState {
                playing: true,
                bluetooth_playing: true
            }
        );
    }

    #[test]
    fn f_cn_07_wired_media_and_silence() {
        let wired = SINKS.replace("SUSPENDED", "RUNNING");
        let s = parse_pactl(&wired, "12\t47\t90\tPipeWire\tfloat32le 2ch 48000Hz\n");
        assert_eq!(
            s,
            OutputState {
                playing: true,
                bluetooth_playing: false
            }
        );
        // A stream on a suspended sink, or none at all, isn't playing.
        assert_eq!(
            parse_pactl(SINKS, "12\t47\t90\tPipeWire\tx\n"),
            OutputState::default()
        );
        assert_eq!(parse_pactl("", ""), OutputState::default());
    }
}
