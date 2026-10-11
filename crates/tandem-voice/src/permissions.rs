//! macOS privacy permissions for Dictate (F-DI-02, F-DI-03, F-DI-06): the
//! microphone, Input Monitoring (the hotkey's event tap) and Accessibility
//! (pasting into the target app). Each is a TCC grant the user gives in
//! System Settings, and an app appears in System Settings' list only after
//! it has asked, so "Grant" first asks (the system prompt) and opens System
//! Settings only once the user has said no (D-2108).
//!
//! The OS sits behind [`PrivacyProbe`] (`macos::MacPrivacy` on a Mac,
//! [`FakePrivacy`] in tests); the decisions are plain functions tested on
//! any OS.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::capture::{AudioBackend, CaptureError, ErrorSink, InputStream, SampleSink};
use crate::devices::DeviceInfo;

#[cfg(target_os = "macos")]
pub mod macos;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Privacy {
    Microphone,
    InputMonitoring,
    Accessibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Granted,
    Denied,
    /// Never asked: the app isn't in System Settings' list yet.
    NotAsked,
}

pub trait PrivacyProbe: Send + Sync {
    fn access(&self, what: Privacy) -> Access;
    /// Shows the system prompt, which also adds the app to System
    /// Settings' list. Returns at once; the answer shows up in `access`.
    fn request(&self, what: Privacy);
}

/// What "Grant" does for a permission in its current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantStep {
    Nothing,
    /// Ask: the system prompt has its own "Open System Settings".
    Prompt,
    /// Asked before and refused: only System Settings can change it.
    OpenSettings,
    /// Accessibility can't tell "never asked" from "refused": ask (which
    /// lists the app) and open the page where its switch is.
    PromptAndOpenSettings,
}

pub fn grant_step(what: Privacy, access: Access) -> GrantStep {
    match (what, access) {
        (_, Access::Granted) => GrantStep::Nothing,
        (Privacy::Accessibility, _) => GrantStep::PromptAndOpenSettings,
        (_, Access::NotAsked) => GrantStep::Prompt,
        (_, Access::Denied) => GrantStep::OpenSettings,
    }
}

/// `IOHIDCheckAccess`: granted (0), denied (1), unknown (2).
pub fn from_iohid(code: u32) -> Access {
    match code {
        0 => Access::Granted,
        1 => Access::Denied,
        _ => Access::NotAsked,
    }
}

/// `AVAuthorizationStatus`: not determined (0), restricted (1, by a profile
/// or parental controls), denied (2), authorized (3).
pub fn from_av_status(status: isize) -> Access {
    match status {
        3 => Access::Granted,
        0 => Access::NotAsked,
        _ => Access::Denied,
    }
}

/// Accessibility has only "trusted" or not.
pub fn from_ax_trusted(trusted: bool) -> Access {
    if trusted {
        Access::Granted
    } else {
        Access::NotAsked
    }
}

/// The system's probe on a Mac; `None` where these permissions don't exist.
pub fn system() -> Option<Arc<dyn PrivacyProbe>> {
    #[cfg(target_os = "macos")]
    {
        Some(Arc::new(macos::MacPrivacy))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// A probe with set answers that records each request.
#[derive(Default)]
pub struct FakePrivacy {
    state: Mutex<Vec<(Privacy, Access)>>,
    pub requests: Mutex<Vec<Privacy>>,
}

impl FakePrivacy {
    pub fn with(entries: &[(Privacy, Access)]) -> Self {
        Self {
            state: Mutex::new(entries.to_vec()),
            requests: Mutex::default(),
        }
    }

    pub fn set(&self, what: Privacy, access: Access) {
        let mut s = self.state.lock();
        s.retain(|(w, _)| *w != what);
        s.push((what, access));
    }
}

impl PrivacyProbe for FakePrivacy {
    fn access(&self, what: Privacy) -> Access {
        self.state
            .lock()
            .iter()
            .find(|(w, _)| *w == what)
            .map(|(_, a)| *a)
            .unwrap_or(Access::NotAsked)
    }

    fn request(&self, what: Privacy) {
        self.requests.lock().push(what);
    }
}

/// The microphone behind its permission. macOS doesn't fail a stream it
/// hasn't been allowed to record: CoreAudio delivers silence. So a take
/// checks first: refused is `PermissionDenied` (the toast's "Allow access"),
/// and never asked shows the system prompt and ends the take the same way,
/// rather than recording seconds of silence while the prompt is up.
pub struct MicGate {
    pub inner: Arc<dyn AudioBackend>,
    pub probe: Arc<dyn PrivacyProbe>,
}

impl AudioBackend for MicGate {
    fn devices(&self) -> Vec<DeviceInfo> {
        self.inner.devices()
    }

    fn open(
        &self,
        device: &str,
        sink: SampleSink,
        on_error: ErrorSink,
    ) -> Result<Box<dyn InputStream>, CaptureError> {
        match self.probe.access(Privacy::Microphone) {
            Access::Granted => self.inner.open(device, sink, on_error),
            Access::Denied => Err(CaptureError::PermissionDenied),
            Access::NotAsked => {
                self.probe.request(Privacy::Microphone);
                Err(CaptureError::PermissionDenied)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::FakeAudioBackend;

    #[test]
    fn f_di_03_grant_asks_first_then_opens_system_settings() {
        use Access::*;
        use GrantStep::*;
        for what in [Privacy::Microphone, Privacy::InputMonitoring] {
            assert_eq!(grant_step(what, Granted), Nothing);
            assert_eq!(grant_step(what, NotAsked), Prompt);
            assert_eq!(grant_step(what, Denied), OpenSettings);
        }
        assert_eq!(grant_step(Privacy::Accessibility, Granted), Nothing);
        assert_eq!(
            grant_step(Privacy::Accessibility, NotAsked),
            PromptAndOpenSettings
        );
    }

    #[test]
    fn f_di_03_os_codes_map_to_access() {
        assert_eq!(from_iohid(0), Access::Granted);
        assert_eq!(from_iohid(1), Access::Denied);
        assert_eq!(from_iohid(2), Access::NotAsked);
        assert_eq!(from_av_status(0), Access::NotAsked);
        assert_eq!(from_av_status(1), Access::Denied, "restricted");
        assert_eq!(from_av_status(2), Access::Denied);
        assert_eq!(from_av_status(3), Access::Granted);
        assert_eq!(from_ax_trusted(true), Access::Granted);
        assert_eq!(from_ax_trusted(false), Access::NotAsked);
    }

    fn open(gate: &MicGate) -> Result<(), CaptureError> {
        gate.open("fake-mic", Box::new(|_| {}), Box::new(|_| {}))
            .map(|_| ())
    }

    #[test]
    fn f_di_03_a_take_never_records_silence_without_the_microphone() {
        let probe = Arc::new(FakePrivacy::default());
        let gate = MicGate {
            inner: Arc::new(FakeAudioBackend::with_samples(vec![0.0; 160], 1)),
            probe: probe.clone(),
        };
        // Never asked: the prompt shows and the take ends as "Allow access".
        assert!(matches!(open(&gate), Err(CaptureError::PermissionDenied)));
        assert_eq!(*probe.requests.lock(), vec![Privacy::Microphone]);
        // Refused: no second prompt.
        probe.set(Privacy::Microphone, Access::Denied);
        assert!(matches!(open(&gate), Err(CaptureError::PermissionDenied)));
        assert_eq!(probe.requests.lock().len(), 1);
        // Allowed: the real stream opens.
        probe.set(Privacy::Microphone, Access::Granted);
        assert!(open(&gate).is_ok());
        assert_eq!(gate.devices().len(), 1);
    }
}
