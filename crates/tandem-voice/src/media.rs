//! "Pause other audio while you talk" (F-DI-03, off by default): the system
//! output is muted for the take and put back afterwards. A marker file under
//! `$DATA` records the state from before, so a crash mid-take is undone on
//! the next launch (AC3). macOS through `osascript`, Linux through `pactl`;
//! not offered on Windows.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MediaError(pub String);

pub trait MediaControl: Send + Sync {
    fn muted(&self) -> Result<bool, MediaError>;
    fn set_muted(&self, muted: bool) -> Result<(), MediaError>;
}

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    prev_muted: bool,
}

pub struct PauseMedia {
    control: Box<dyn MediaControl>,
    marker: PathBuf,
}

impl PauseMedia {
    pub fn new(control: Box<dyn MediaControl>, marker: PathBuf) -> Self {
        Self { control, marker }
    }

    /// At launch: a marker means the last run muted and never unmuted.
    pub fn recover(&self) -> bool {
        let Ok(raw) = std::fs::read(&self.marker) else {
            return false;
        };
        let prev = serde_json::from_slice::<Marker>(&raw)
            .map(|m| m.prev_muted)
            .unwrap_or(false);
        let ok = self.control.set_muted(prev).is_ok();
        let _ = std::fs::remove_file(&self.marker);
        ok
    }

    /// Mutes for a take (the marker is written first).
    pub fn begin(&self) -> Result<(), MediaError> {
        let prev = self.control.muted()?;
        if prev {
            return Ok(()); // already silent: nothing to undo
        }
        let marker = serde_json::to_vec(&Marker { prev_muted: prev })
            .map_err(|e| MediaError(e.to_string()))?;
        tandem_core::atomic::atomic_write(&self.marker, &marker, None)
            .map_err(|e| MediaError(e.to_string()))?;
        self.control.set_muted(true)
    }

    /// Puts the output back as it was.
    pub fn end(&self) {
        self.recover();
    }
}

/// The system's control, where there is one.
pub fn native() -> Option<Box<dyn MediaControl>> {
    if cfg!(target_os = "linux") {
        Some(Box::new(Pactl))
    } else if cfg!(target_os = "macos") {
        Some(Box::new(Osascript))
    } else {
        None
    }
}

struct Pactl;

impl MediaControl for Pactl {
    fn muted(&self) -> Result<bool, MediaError> {
        let out = Command::new("pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .output()
            .map_err(|e| MediaError(e.to_string()))?;
        if !out.status.success() {
            return Err(MediaError("pactl failed".into()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).contains("yes"))
    }

    fn set_muted(&self, muted: bool) -> Result<(), MediaError> {
        let ok = Command::new("pactl")
            .args([
                "set-sink-mute",
                "@DEFAULT_SINK@",
                if muted { "1" } else { "0" },
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        ok.then_some(())
            .ok_or_else(|| MediaError("pactl failed".into()))
    }
}

struct Osascript;

impl MediaControl for Osascript {
    fn muted(&self) -> Result<bool, MediaError> {
        let out = Command::new("osascript")
            .args(["-e", "output muted of (get volume settings)"])
            .output()
            .map_err(|e| MediaError(e.to_string()))?;
        Ok(String::from_utf8_lossy(&out.stdout).trim() == "true")
    }

    fn set_muted(&self, muted: bool) -> Result<(), MediaError> {
        let script = if muted {
            "set volume output muted true"
        } else {
            "set volume output muted false"
        };
        let ok = Command::new("osascript")
            .args(["-e", script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        ok.then_some(())
            .ok_or_else(|| MediaError("osascript failed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Fake(Arc<parking_lot::Mutex<bool>>);
    impl MediaControl for Fake {
        fn muted(&self) -> Result<bool, MediaError> {
            Ok(*self.0.lock())
        }
        fn set_muted(&self, m: bool) -> Result<(), MediaError> {
            *self.0.lock() = m;
            Ok(())
        }
    }

    #[test]
    fn f_di_03_3_a_crash_while_muted_is_undone_on_the_next_launch() {
        let dir = tempfile::tempdir().expect("dir");
        let marker = dir.path().join("audio-mute.marker");
        let sys = Fake::default();
        let first = PauseMedia::new(Box::new(sys.clone()), marker.clone());
        first.begin().expect("mute");
        assert!(*sys.0.lock(), "muted during the take");
        assert!(marker.exists());
        drop(first); // the app crashes here: end() never runs
        let next_launch = PauseMedia::new(Box::new(sys.clone()), marker.clone());
        assert!(next_launch.recover());
        assert!(!*sys.0.lock(), "unmuted on the next launch");
        assert!(!marker.exists());
        assert!(!next_launch.recover(), "nothing to do twice");
    }

    #[test]
    fn f_di_03_already_muted_output_stays_muted() {
        let dir = tempfile::tempdir().expect("dir");
        let marker = dir.path().join("audio-mute.marker");
        let sys = Fake::default();
        *sys.0.lock() = true;
        let p = PauseMedia::new(Box::new(sys.clone()), marker.clone());
        p.begin().expect("begin");
        assert!(!marker.exists());
        p.end();
        assert!(*sys.0.lock());
    }
}
