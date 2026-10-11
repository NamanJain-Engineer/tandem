//! A [`WakeDetector`] backed by the `tandem-wake` sidecar (see
//! `src/bin/tandem-wake.rs` for the frame protocol). Detections arrive on a
//! reader thread; `accept` reports any that came in since the last call.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Sensitivity, WakeDetector};

/// The sidecar's file name beside the app's executable.
pub const BIN_NAME: &str = if cfg!(windows) {
    "tandem-wake.exe"
} else {
    "tandem-wake"
};

/// `tandem-wake` beside `exe`, or the debug override.
pub fn sidecar_from(dev: Option<PathBuf>, exe: &Path) -> Option<PathBuf> {
    if let Some(p) = dev {
        return p.is_file().then_some(p);
    }
    let p = exe.parent()?.join(BIN_NAME);
    p.is_file().then_some(p)
}

/// The sidecar for this app (`TANDEM_WAKE_BIN` in debug builds).
pub fn sidecar() -> Option<PathBuf> {
    let dev = if cfg!(debug_assertions) {
        std::env::var_os("TANDEM_WAKE_BIN").map(PathBuf::from)
    } else {
        None
    };
    sidecar_from(dev, &std::env::current_exe().ok()?)
}

pub struct ProcessDetector {
    child: Child,
    stdin: ChildStdin,
    wakes: Arc<AtomicUsize>,
    seen: usize,
    alive: Arc<AtomicBool>,
}

impl ProcessDetector {
    /// Starts the sidecar and waits (up to 10 s) for it to load the model.
    pub fn spawn(bin: &Path, model: &Path, sensitivity: Sensitivity) -> Result<Self, String> {
        let s = match sensitivity {
            Sensitivity::Low => "low",
            Sensitivity::Medium => "medium",
            Sensitivity::High => "high",
        };
        let mut child = Command::new(bin)
            .arg("--model")
            .arg(model)
            .args(["--sensitivity", s])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("the wake detector didn't start: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let wakes = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let (w, r, a) = (wakes.clone(), ready.clone(), alive.clone());
        std::thread::Builder::new()
            .name("wake-sidecar".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    match line.as_deref() {
                        Ok("ready") => r.store(true, Ordering::Release),
                        Ok("wake") => {
                            w.fetch_add(1, Ordering::AcqRel);
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                a.store(false, Ordering::Release);
            })
            .map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(10);
        while !ready.load(Ordering::Acquire) {
            if !alive.load(Ordering::Acquire) || Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the wake model didn't load".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(Self {
            child,
            stdin,
            wakes,
            seen: 0,
            alive,
        })
    }

    fn send(&mut self, samples: &[f32]) {
        let mut frame = Vec::with_capacity(4 + samples.len() * 4);
        frame.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        for s in samples {
            frame.extend_from_slice(&s.to_le_bytes());
        }
        if self
            .stdin
            .write_all(&frame)
            .and_then(|_| self.stdin.flush())
            .is_err()
        {
            self.alive.store(false, Ordering::Release);
        }
    }
}

impl WakeDetector for ProcessDetector {
    fn accept(&mut self, samples: &[f32]) -> bool {
        for chunk in samples.chunks(16_000) {
            self.send(chunk);
        }
        let now = self.wakes.load(Ordering::Acquire);
        let heard = now > self.seen;
        self.seen = now;
        heard
    }

    fn reset(&mut self) {
        self.send(&[]);
        self.seen = self.wakes.load(Ordering::Acquire);
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
}

impl Drop for ProcessDetector {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_07_sidecar_is_found_beside_the_app_or_by_override() {
        let dir = tempfile::tempdir().expect("test");
        let exe = dir.path().join("tandem");
        assert_eq!(sidecar_from(None, &exe), None);
        std::fs::write(dir.path().join(BIN_NAME), b"").expect("test");
        assert_eq!(sidecar_from(None, &exe), Some(dir.path().join(BIN_NAME)));
        assert_eq!(sidecar_from(Some(dir.path().join("missing")), &exe), None);
    }
}
