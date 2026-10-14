//! Global push-to-talk (F-DI-01, F-DI-02, ARCHITECTURE §12.2): bindings, the
//! gesture logic and one listen-only backend per OS behind `HotkeyBackend`.

pub mod binding;
pub mod gesture;
pub mod keymaps;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

use std::collections::HashSet;
use std::sync::mpsc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use binding::{Binding, BindingError, Os};
pub use gesture::{Action, Gesture, KeyEvent, Phase, Verifier, Which};

/// The Settings › Dictate › Shortcut status chip (UI-62).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum HotkeyStatus {
    Active,
    /// macOS: "Needs Input Monitoring [Grant]".
    NeedsPermission,
    /// No way to listen here (Wayland without the portal, …).
    Unavailable {
        reason: String,
    },
    /// macOS secure input (a password field has the keyboard).
    Paused,
    Starting,
    NotRunning,
}

/// What a backend listens for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    pub hold: Option<Binding>,
    pub toggle: Option<Binding>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("Keyboard hook failed — Dictate unavailable")]
    Failed(String),
    #[error("This system doesn't let apps listen for a global shortcut")]
    Unavailable(String),
    #[error("Tandem needs Input Monitoring to hear your shortcut")]
    NeedsPermission,
}

/// A source of key events for the bound shortcuts. Implementations never
/// consume events: other apps see every key as if Tandem weren't there.
pub trait HotkeyBackend: Send + Sync {
    /// Starts (or restarts) listening for `bindings`. Events go to `tx`.
    fn start(
        &self,
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
    ) -> Result<HotkeyStatus, HotkeyError>;
    fn stop(&self);
    /// Esc is only listened for while a take records.
    fn listen_esc(&self, on: bool);
    fn status(&self) -> HotkeyStatus;
}

/// Milliseconds since the first call, for `KeyEvent::at_ms`.
pub fn now_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The fake hotkey source (MILESTONES M8 "a fake hotkey event source"): tests
/// and the E2E harness press and release the bound shortcut through it.
#[derive(Default)]
pub struct FakeHotkeys {
    tx: Mutex<Option<mpsc::Sender<KeyEvent>>>,
    bindings: Mutex<Option<Bindings>>,
    esc: Mutex<bool>,
    /// What the backend reports, when a test sets it (secure input, a
    /// session that died).
    status: Mutex<Option<HotkeyStatus>>,
    /// Runs inside the next `status` call, before it answers (a test making
    /// something happen while the status is being read).
    next_status: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl FakeHotkeys {
    pub fn send(&self, which: Which, phase: Phase) -> bool {
        if which == Which::Esc && !*self.esc.lock() {
            return false;
        }
        let tx = self.tx.lock();
        let Some(tx) = tx.as_ref() else { return false };
        tx.send(KeyEvent {
            which,
            phase,
            native: true,
            at_ms: now_ms(),
        })
        .is_ok()
    }

    pub fn bindings(&self) -> Option<Bindings> {
        self.bindings.lock().clone()
    }

    pub fn esc_listening(&self) -> bool {
        *self.esc.lock()
    }

    /// Makes the backend report `status` from now on.
    pub fn set_status(&self, status: HotkeyStatus) {
        *self.status.lock() = Some(status);
    }

    /// Runs `f` inside the next `status` call, before it answers.
    pub fn on_next_status(&self, f: impl FnOnce() + Send + 'static) {
        *self.next_status.lock() = Some(Box::new(f));
    }
}

impl HotkeyBackend for FakeHotkeys {
    fn start(
        &self,
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
    ) -> Result<HotkeyStatus, HotkeyError> {
        *self.tx.lock() = Some(tx);
        *self.bindings.lock() = Some(bindings.clone());
        Ok(HotkeyStatus::Active)
    }

    fn stop(&self) {
        *self.tx.lock() = None;
    }

    fn listen_esc(&self, on: bool) {
        *self.esc.lock() = on;
    }

    fn status(&self) -> HotkeyStatus {
        let hook = self.next_status.lock().take();
        if let Some(f) = hook {
            f();
        }
        if let Some(s) = self.status.lock().clone() {
            return s;
        }
        if self.tx.lock().is_some() {
            HotkeyStatus::Active
        } else {
            HotkeyStatus::NotRunning
        }
    }
}

/// The backend for this OS.
pub fn native() -> std::sync::Arc<dyn HotkeyBackend> {
    #[cfg(target_os = "linux")]
    {
        std::sync::Arc::new(linux::LinuxHotkeys::new())
    }
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos::MacEventTap::new())
    }
    #[cfg(windows)]
    {
        std::sync::Arc::new(windows::WinLowLevelHook::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        std::sync::Arc::new(Unsupported)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct Unsupported;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl HotkeyBackend for Unsupported {
    fn start(&self, _: &Bindings, _: mpsc::Sender<KeyEvent>) -> Result<HotkeyStatus, HotkeyError> {
        Err(HotkeyError::Unavailable("unsupported OS".into()))
    }
    fn stop(&self) {}
    fn listen_esc(&self, _: bool) {}
    fn status(&self) -> HotkeyStatus {
        HotkeyStatus::Unavailable {
            reason: "unsupported OS".into(),
        }
    }
}

/// Tracks which bound keys are down from raw key transitions, for backends
/// that see individual keys (X11, the Windows hook, the macOS tap): emits a
/// Down when the whole combination becomes pressed and an Up when it stops
/// being pressed.
#[derive(Debug, Clone)]
pub struct ComboTracker {
    binding: Binding,
    which: Which,
    pressed: bool,
    interrupted: bool,
}

/// A raw key in OS-neutral terms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RawKey {
    Modifier(binding::Modifier),
    Key(String),
    Mouse4,
    Mouse5,
}

impl ComboTracker {
    pub fn new(binding: Binding, which: Which) -> Self {
        Self {
            binding,
            which,
            pressed: false,
            interrupted: false,
        }
    }

    /// `held` is every key down after this transition; `extra_keys` says
    /// whether keys outside the binding are down (a modifier-only binding
    /// only fires on its own: Ctrl+Alt+T is a different shortcut).
    pub fn update(&mut self, held: &HashSet<RawKey>, at_ms: u64) -> Option<KeyEvent> {
        let mods_ok = self
            .binding
            .modifiers
            .iter()
            .all(|m| held.contains(&RawKey::Modifier(*m)));
        let trigger_ok = match &self.binding.trigger {
            None => true,
            Some(binding::Trigger::Key(k)) => held.contains(&RawKey::Key(k.clone())),
            Some(binding::Trigger::Mouse4) => held.contains(&RawKey::Mouse4),
            Some(binding::Trigger::Mouse5) => held.contains(&RawKey::Mouse5),
        };
        let extra = held.iter().any(|k| match k {
            RawKey::Modifier(m) => !self.binding.modifiers.contains(m),
            RawKey::Key(name) => {
                !matches!(&self.binding.trigger, Some(binding::Trigger::Key(t)) if t == name)
            }
            RawKey::Mouse4 | RawKey::Mouse5 => false,
        });
        // Holding a modifier-only shortcut and pressing another key (Ctrl+Alt+T)
        // was another shortcut all along: the take is cancelled, once.
        let other_key = held.iter().any(|k| matches!(k, RawKey::Key(_)));
        if self.pressed && self.binding.is_modifier_only() && other_key && !self.interrupted {
            self.interrupted = true;
            return Some(KeyEvent {
                which: Which::Esc,
                phase: Phase::Down,
                native: true,
                at_ms,
            });
        }
        let now = mods_ok && trigger_ok && (!self.pressed && !extra || self.pressed);
        if !now {
            self.interrupted = false;
        }
        let down = now && !self.pressed;
        let up = !now && self.pressed;
        self.pressed = now;
        (down || up).then_some(KeyEvent {
            which: self.which,
            phase: if down { Phase::Down } else { Phase::Up },
            native: true,
            at_ms,
        })
    }
}

/// Feeds raw transitions to the bound combinations and Esc.
pub struct KeyState {
    held: HashSet<RawKey>,
    trackers: Vec<ComboTracker>,
}

impl KeyState {
    pub fn new(bindings: &Bindings) -> Self {
        let mut trackers = Vec::new();
        if let Some(b) = &bindings.hold {
            trackers.push(ComboTracker::new(b.clone(), Which::Hold));
        }
        if let Some(b) = &bindings.toggle {
            trackers.push(ComboTracker::new(b.clone(), Which::Toggle));
        }
        Self {
            held: HashSet::new(),
            trackers,
        }
    }

    pub fn transition(&mut self, key: RawKey, down: bool, esc: bool, at_ms: u64) -> Vec<KeyEvent> {
        let mut out = Vec::new();
        if esc && down && key == RawKey::Key("Esc".into()) {
            out.push(KeyEvent {
                which: Which::Esc,
                phase: Phase::Down,
                native: true,
                at_ms,
            });
        }
        if down {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        for t in &mut self.trackers {
            if let Some(e) = t.update(&self.held, at_ms) {
                out.push(e);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use binding::Modifier;

    fn held(keys: &[RawKey]) -> HashSet<RawKey> {
        keys.iter().cloned().collect()
    }

    #[test]
    fn f_di_02_combo_fires_once_on_press_and_once_on_release() {
        let b: Binding = "Ctrl+Alt".parse().expect("b");
        let mut t = ComboTracker::new(b, Which::Hold);
        let ctrl = RawKey::Modifier(Modifier::Ctrl);
        let alt = RawKey::Modifier(Modifier::Alt);
        assert_eq!(t.update(&held(std::slice::from_ref(&ctrl)), 0), None);
        let d = t
            .update(&held(&[ctrl.clone(), alt.clone()]), 5)
            .expect("down");
        assert_eq!(d.phase, Phase::Down);
        // Another key joining mid-hold cancels it (once) rather than
        // releasing it; the release still comes when the keys go up.
        assert_eq!(
            t.update(
                &held(&[ctrl.clone(), alt.clone(), RawKey::Key("T".into())]),
                9
            )
            .map(|e| e.which),
            Some(Which::Esc)
        );
        let u = t.update(&held(std::slice::from_ref(&alt)), 20).expect("up");
        assert_eq!(u.phase, Phase::Up);
        assert_eq!(t.update(&held(&[]), 30), None);
    }

    #[test]
    fn f_di_01_another_key_during_a_modifier_only_hold_cancels_it() {
        let b: Binding = "Ctrl+Alt".parse().expect("b");
        let mut t = ComboTracker::new(b, Which::Hold);
        let ctrl = RawKey::Modifier(Modifier::Ctrl);
        let alt = RawKey::Modifier(Modifier::Alt);
        let tkey = RawKey::Key("T".into());
        assert_eq!(
            t.update(&held(&[ctrl.clone(), alt.clone()]), 0)
                .map(|e| e.phase),
            Some(Phase::Down)
        );
        let e = t
            .update(&held(&[ctrl.clone(), alt.clone(), tkey.clone()]), 50)
            .expect("esc");
        assert_eq!((e.which, e.phase), (Which::Esc, Phase::Down));
        assert_eq!(
            t.update(&held(&[ctrl.clone(), alt.clone()]), 60),
            None,
            "only once"
        );
        assert_eq!(t.update(&held(&[]), 70).map(|e| e.phase), Some(Phase::Up));
        // A combo with its own key isn't interrupted by that key.
        let mut k = ComboTracker::new("Ctrl+Alt+D".parse().expect("b"), Which::Toggle);
        let d = RawKey::Key("D".into());
        assert!(k
            .update(&held(&[ctrl.clone(), alt.clone(), d.clone()]), 0)
            .is_some());
        assert_eq!(k.update(&held(&[ctrl, alt, d]), 5), None);
    }

    #[test]
    fn f_di_02_modifier_only_ignores_other_shortcuts() {
        let b: Binding = "Ctrl+Alt".parse().expect("b");
        let mut t = ComboTracker::new(b, Which::Hold);
        let ctrl = RawKey::Modifier(Modifier::Ctrl);
        let alt = RawKey::Modifier(Modifier::Alt);
        let tkey = RawKey::Key("T".into());
        t.update(&held(std::slice::from_ref(&ctrl)), 0);
        t.update(&held(&[ctrl.clone(), tkey.clone()]), 1);
        assert_eq!(
            t.update(&held(&[ctrl, tkey, alt]), 2),
            None,
            "Ctrl+T then Alt isn't Ctrl+Alt"
        );
    }

    #[test]
    fn f_di_02_fake_source_delivers_esc_only_while_listening() {
        let fake = FakeHotkeys::default();
        let (tx, rx) = mpsc::channel();
        let b = Bindings {
            hold: Some(Binding::default_hold(Os::Linux)),
            toggle: None,
        };
        assert_eq!(fake.start(&b, tx), Ok(HotkeyStatus::Active));
        assert!(fake.send(Which::Hold, Phase::Down));
        assert!(!fake.send(Which::Esc, Phase::Down));
        fake.listen_esc(true);
        assert!(fake.send(Which::Esc, Phase::Down));
        let got: Vec<Which> = rx.try_iter().map(|e| e.which).collect();
        assert_eq!(got, vec![Which::Hold, Which::Esc]);
    }

    /// F-DI-02 AC2: the Windows hook only listens. The guard script (also a
    /// CI step) passes on the real file and catches a consuming return.
    #[cfg(unix)]
    #[test]
    fn f_di_02_2_the_windows_hook_never_consumes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let guard = root.join("scripts/guards/no-consuming-hook.sh");
        let real = root.join("crates/tandem-voice/src/hotkey/windows.rs");
        let run = |file: &std::path::Path| {
            std::process::Command::new("bash")
                .arg(&guard)
                .arg(file)
                .output()
                .expect("bash")
                .status
                .success()
        };
        assert!(run(&real), "the hook file consumes an event");
        let source = std::fs::read_to_string(&real).expect("read");
        let dir = tempfile::tempdir().expect("dir");
        let bad = dir.path().join("windows.rs");
        let consuming = source.replacen(
            "unsafe { CallNextHookEx(None, code, wparam, lparam) }",
            "LRESULT(1)",
            1,
        );
        std::fs::write(&bad, consuming).expect("write");
        assert!(!run(&bad), "the guard missed a consuming return");
    }
}
