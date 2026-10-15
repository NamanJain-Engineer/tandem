//! macOS's "Press 🌐 key to" preference (F-DI-11): with Fn as the Dictate
//! key, a 🌐 press also opens the emoji picker or switches the input source
//! unless the preference is "Do Nothing". The app changes it only after the
//! user agrees, and puts the old value back when asked.
//!
//! The preference is `AppleFnUsageType` in the `com.apple.HIToolbox` domain,
//! read and written with `/usr/bin/defaults`. The parsing is shared and
//! tested everywhere; only the command runner is macOS's.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GlobeUsage {
    DoNothing,
    ChangeInputSource,
    ShowEmoji,
    StartDictation,
}

impl GlobeUsage {
    pub fn from_defaults(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::DoNothing),
            1 => Some(Self::ChangeInputSource),
            2 => Some(Self::ShowEmoji),
            3 => Some(Self::StartDictation),
            _ => None,
        }
    }

    pub fn to_defaults(self) -> i64 {
        match self {
            Self::DoNothing => 0,
            Self::ChangeInputSource => 1,
            Self::ShowEmoji => 2,
            Self::StartDictation => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GlobeError {
    #[error("This preference exists only on macOS")]
    Unsupported,
    #[error("Couldn't change the 🌐 key setting: {0}")]
    Failed(String),
}

/// The preference, behind the platform trait.
pub trait GlobeKey: Send + Sync {
    fn available(&self) -> bool;
    /// `None`: never set (the system's default applies).
    fn read(&self) -> Result<Option<GlobeUsage>, GlobeError>;
    /// `None` removes it, back to the system's default.
    fn write(&self, usage: Option<GlobeUsage>) -> Result<(), GlobeError>;
}

/// One run of a command: exit success, stdout, stderr.
pub type Run = Arc<dyn Fn(&str, &[&str]) -> Result<(bool, String, String), String> + Send + Sync>;

const DEFAULTS: &str = "/usr/bin/defaults";
const DOMAIN: &str = "com.apple.HIToolbox";
const KEY: &str = "AppleFnUsageType";
/// Applies a changed preference without logging out.
const ACTIVATE: &str =
    "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings";

/// `defaults` through a runner (the real one on macOS, a fake in tests).
pub struct DefaultsGlobe {
    run: Run,
}

impl DefaultsGlobe {
    pub fn new(run: Run) -> Self {
        Self { run }
    }
}

impl GlobeKey for DefaultsGlobe {
    fn available(&self) -> bool {
        true
    }

    fn read(&self) -> Result<Option<GlobeUsage>, GlobeError> {
        let (ok, out, err) =
            (self.run)(DEFAULTS, &["read", DOMAIN, KEY]).map_err(GlobeError::Failed)?;
        if !ok {
            // "The domain/default pair … does not exist": never set.
            return if err.contains("does not exist") {
                Ok(None)
            } else {
                Err(GlobeError::Failed(err.trim().to_string()))
            };
        }
        Ok(out
            .trim()
            .parse::<i64>()
            .ok()
            .and_then(GlobeUsage::from_defaults))
    }

    fn write(&self, usage: Option<GlobeUsage>) -> Result<(), GlobeError> {
        let value = usage.map(|u| u.to_defaults().to_string());
        let args: Vec<&str> = match &value {
            Some(v) => vec!["write", DOMAIN, KEY, "-int", v.as_str()],
            None => vec!["delete", DOMAIN, KEY],
        };
        let (ok, _, err) = (self.run)(DEFAULTS, &args).map_err(GlobeError::Failed)?;
        // Deleting a key that isn't there is already the wanted state.
        if !ok && !(usage.is_none() && err.contains("does not exist")) {
            return Err(GlobeError::Failed(err.trim().to_string()));
        }
        // Best effort: without it the change applies at the next login.
        let _ = (self.run)(ACTIVATE, &["-u"]);
        Ok(())
    }
}

/// Every other OS.
pub struct NoGlobe;

impl GlobeKey for NoGlobe {
    fn available(&self) -> bool {
        false
    }
    fn read(&self) -> Result<Option<GlobeUsage>, GlobeError> {
        Err(GlobeError::Unsupported)
    }
    fn write(&self, _usage: Option<GlobeUsage>) -> Result<(), GlobeError> {
        Err(GlobeError::Unsupported)
    }
}

/// The E2E harness's and the tests' preference.
#[derive(Default)]
pub struct FakeGlobe {
    pub value: Mutex<Option<GlobeUsage>>,
    pub writes: Mutex<Vec<Option<GlobeUsage>>>,
}

impl GlobeKey for FakeGlobe {
    fn available(&self) -> bool {
        true
    }
    fn read(&self) -> Result<Option<GlobeUsage>, GlobeError> {
        Ok(*self.value.lock())
    }
    fn write(&self, usage: Option<GlobeUsage>) -> Result<(), GlobeError> {
        *self.value.lock() = usage;
        self.writes.lock().push(usage);
        Ok(())
    }
}

pub fn native() -> Arc<dyn GlobeKey> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(DefaultsGlobe::new(Arc::new(|cmd: &str, args: &[&str]| {
            std::process::Command::new(cmd)
                .args(args)
                .output()
                .map(|o| {
                    (
                        o.status.success(),
                        String::from_utf8_lossy(&o.stdout).into_owned(),
                        String::from_utf8_lossy(&o.stderr).into_owned(),
                    )
                })
                .map_err(|e| e.to_string())
        })))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(NoGlobe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Calls = Arc<Mutex<Vec<String>>>;

    /// A `defaults` that keeps one value, and logs each command.
    fn fake() -> (DefaultsGlobe, Calls) {
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let store: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let log = calls.clone();
        let run: Run = Arc::new(move |cmd: &str, args: &[&str]| {
            log.lock().push(format!("{cmd} {}", args.join(" ")));
            let missing = (
                false,
                String::new(),
                "The domain/default pair of (com.apple.HIToolbox, AppleFnUsageType) does not exist"
                    .to_string(),
            );
            Ok(match args.first().copied() {
                Some("read") => match store.lock().clone() {
                    Some(v) => (true, format!("{v}\n"), String::new()),
                    None => missing,
                },
                Some("write") => {
                    *store.lock() = args.get(4).map(|v| v.to_string());
                    (true, String::new(), String::new())
                }
                Some("delete") => match store.lock().take() {
                    Some(_) => (true, String::new(), String::new()),
                    None => missing,
                },
                _ => (true, String::new(), String::new()),
            })
        });
        (DefaultsGlobe::new(run), calls)
    }

    #[test]
    fn f_di_11_the_globe_preference_reads_writes_and_goes_back_to_unset() {
        let (g, calls) = fake();
        assert_eq!(g.read(), Ok(None));
        g.write(Some(GlobeUsage::DoNothing)).expect("write");
        assert_eq!(g.read(), Ok(Some(GlobeUsage::DoNothing)));
        assert!(calls.lock().contains(
            &"/usr/bin/defaults write com.apple.HIToolbox AppleFnUsageType -int 0".to_string()
        ));
        g.write(Some(GlobeUsage::ShowEmoji)).expect("write");
        assert_eq!(g.read(), Ok(Some(GlobeUsage::ShowEmoji)));
        g.write(None).expect("delete");
        assert_eq!(g.read(), Ok(None));
        // Deleting what isn't there is fine.
        g.write(None).expect("delete again");
        // Each change is applied without a logout.
        assert!(calls
            .lock()
            .iter()
            .any(|c| c.ends_with("activateSettings -u")));
    }

    #[test]
    fn f_di_11_a_failing_defaults_is_an_error_not_a_guess() {
        let g = DefaultsGlobe::new(Arc::new(|_: &str, _: &[&str]| {
            Ok((false, String::new(), "Could not write domain".to_string()))
        }));
        assert!(matches!(g.read(), Err(GlobeError::Failed(_))));
        assert!(matches!(
            g.write(Some(GlobeUsage::DoNothing)),
            Err(GlobeError::Failed(_))
        ));
        assert!(!NoGlobe.available());
        assert_eq!(NoGlobe.read(), Err(GlobeError::Unsupported));
    }

    #[test]
    fn f_di_11_usage_values_match_the_system_menu() {
        for u in [
            GlobeUsage::DoNothing,
            GlobeUsage::ChangeInputSource,
            GlobeUsage::ShowEmoji,
            GlobeUsage::StartDictation,
        ] {
            assert_eq!(GlobeUsage::from_defaults(u.to_defaults()), Some(u));
        }
        assert_eq!(GlobeUsage::from_defaults(9), None);
    }
}
