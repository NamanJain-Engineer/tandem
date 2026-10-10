//! `FakePlatform` (MILESTONES M8): records focus, paste and clipboard calls,
//! and keeps what each paste would have typed, like a test text field.

use parking_lot::Mutex;

use super::{Clip, InsertError, MissingTools, Platform, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    CaptureTarget,
    RestoreFocus(u64),
    Paste { terminal: bool },
    ReadClipboard,
    WriteClipboard(String),
    RestoreClipboard,
}

struct Inner {
    calls: Vec<Call>,
    clipboard: Clip,
    pasted: Vec<String>,
    target: Option<Target>,
    fail_focus: bool,
    fail_paste: bool,
    restores: bool,
    can_capture: bool,
    missing: Option<MissingTools>,
}

pub struct FakePlatform {
    inner: Mutex<Inner>,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                calls: Vec::new(),
                clipboard: Clip::Empty,
                pasted: Vec::new(),
                target: None,
                fail_focus: false,
                fail_paste: false,
                restores: true,
                can_capture: true,
                missing: None,
            }),
        }
    }
}

impl FakePlatform {
    pub fn calls(&self) -> Vec<Call> {
        self.inner.lock().calls.clone()
    }
    pub fn clear_calls(&self) {
        let mut i = self.inner.lock();
        i.calls.clear();
        i.pasted.clear();
    }
    /// The text each paste delivered to the focused field.
    pub fn pasted(&self) -> Vec<String> {
        self.inner.lock().pasted.clone()
    }
    pub fn clipboard(&self) -> Clip {
        self.inner.lock().clipboard.clone()
    }
    pub fn set_clipboard(&self, clip: Clip) {
        self.inner.lock().clipboard = clip;
    }
    pub fn set_target(&self, target: Option<Target>) {
        self.inner.lock().target = target;
    }
    pub fn fail_focus(&self, on: bool) {
        self.inner.lock().fail_focus = on;
    }
    pub fn fail_paste(&self, on: bool) {
        self.inner.lock().fail_paste = on;
    }
    pub fn set_restores(&self, on: bool) {
        self.inner.lock().restores = on;
    }
    pub fn set_can_capture(&self, on: bool) {
        self.inner.lock().can_capture = on;
    }
    pub fn set_missing(&self, m: Option<MissingTools>) {
        self.inner.lock().missing = m;
    }
}

impl Platform for FakePlatform {
    fn capture_target(&self) -> Result<Option<Target>, InsertError> {
        let mut i = self.inner.lock();
        i.calls.push(Call::CaptureTarget);
        Ok(i.target.clone())
    }

    fn restore_focus(&self, target: &Target) -> Result<(), InsertError> {
        let mut i = self.inner.lock();
        i.calls.push(Call::RestoreFocus(target.window));
        if i.fail_focus {
            Err(InsertError::Focus)
        } else {
            Ok(())
        }
    }

    fn paste(&self, terminal: bool) -> Result<(), InsertError> {
        let mut i = self.inner.lock();
        i.calls.push(Call::Paste { terminal });
        if i.fail_paste {
            return Err(InsertError::Paste("fake".into()));
        }
        if let Clip::Text(t) = i.clipboard.clone() {
            i.pasted.push(t);
        }
        Ok(())
    }

    fn read_clipboard(&self) -> Result<Clip, InsertError> {
        let mut i = self.inner.lock();
        i.calls.push(Call::ReadClipboard);
        Ok(i.clipboard.clone())
    }

    fn write_clipboard(&self, text: &str) -> Result<(), InsertError> {
        let mut i = self.inner.lock();
        i.calls.push(Call::WriteClipboard(text.into()));
        i.clipboard = Clip::Text(text.into());
        Ok(())
    }

    fn restore_clipboard(&self, clip: &Clip) -> Result<(), InsertError> {
        let mut i = self.inner.lock();
        i.calls.push(Call::RestoreClipboard);
        i.clipboard = clip.clone();
        Ok(())
    }

    fn restores_clipboard(&self) -> bool {
        self.inner.lock().restores
    }

    fn can_capture(&self) -> bool {
        self.inner.lock().can_capture
    }

    fn missing_tools(&self) -> Option<MissingTools> {
        self.inner.lock().missing.clone()
    }
}
