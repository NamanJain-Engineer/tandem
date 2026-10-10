//! Putting a take's text where the cursor was (F-DI-06, ARCHITECTURE §12.5).
//!
//! The target is captured when recording starts. At the end: the text goes
//! on the clipboard, focus goes back to the target, Mod+V is synthesised,
//! and (macOS, Linux) the previous clipboard comes back 500 ms later. Any
//! doubt fails closed: the text stays on the clipboard and the user is told
//! to paste it ("Copied — press {Mod}+V to paste").

pub mod fake;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::postproc;

/// How long the pasted text stays on the clipboard before the old content
/// comes back.
pub const RESTORE_AFTER: Duration = Duration::from_millis(500);

/// The window and field a take was started over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "InsertTarget")]
pub struct Target {
    /// Bundle id (macOS), executable name (Windows), WM class (X11).
    pub app_id: Option<String>,
    /// What the UI calls the app ("TextEdit").
    pub app_name: Option<String>,
    /// The OS handle, opaque to everything but the platform.
    #[ts(type = "string")]
    pub window: u64,
    /// The focused element inside it, where the OS names one.
    #[ts(type = "string")]
    pub element: u64,
    /// A terminal: control characters stripped, never an Enter.
    pub terminal: bool,
    /// Windows UIPI: the app runs elevated and can't receive input from us.
    pub elevated: bool,
    /// One of Tandem's own windows (never an external target).
    pub own: bool,
}

/// What was on the clipboard before the paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clip {
    Empty,
    Text(String),
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InsertError {
    #[error("the target window is gone")]
    TargetGone,
    #[error("focus couldn't be given back to the target")]
    Focus,
    #[error("the paste couldn't be typed: {0}")]
    Paste(String),
    #[error("the clipboard isn't available: {0}")]
    Clipboard(String),
    #[error("{0}")]
    Unsupported(String),
}

/// Typing tools Linux needs and lacks, with the commands that install them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MissingTools {
    pub tools: Vec<String>,
    pub apt: String,
    pub dnf: String,
    pub pacman: String,
}

impl MissingTools {
    pub fn new(tools: &[&str]) -> Self {
        let list = tools.join(" ");
        Self {
            tools: tools.iter().map(|t| t.to_string()).collect(),
            apt: format!("sudo apt install {list}"),
            dnf: format!("sudo dnf install {list}"),
            pacman: format!("sudo pacman -S {list}"),
        }
    }
}

/// One OS's window, keyboard and clipboard access.
pub trait Platform: Send + Sync {
    /// The focused window and field, or `None` where the OS doesn't say
    /// (Wayland).
    fn capture_target(&self) -> Result<Option<Target>, InsertError>;
    /// Gives focus back to `target` and confirms it has it.
    fn restore_focus(&self, target: &Target) -> Result<(), InsertError>;
    /// Synthesises the paste shortcut (Ctrl+Shift+V in a Linux terminal).
    fn paste(&self, terminal: bool) -> Result<(), InsertError>;
    fn read_clipboard(&self) -> Result<Clip, InsertError>;
    fn write_clipboard(&self, text: &str) -> Result<(), InsertError>;
    fn restore_clipboard(&self, clip: &Clip) -> Result<(), InsertError>;
    /// macOS and Linux restore the previous clipboard; Windows doesn't, so a
    /// clipboard-history app can't capture the text twice.
    fn restores_clipboard(&self) -> bool;
    /// Undoes the last edit in the focused field (Mod+Z): a quick action
    /// replacing the text it just pasted (F-DI-10).
    fn undo(&self) -> Result<(), InsertError> {
        Err(InsertError::Paste("undo isn't available here".into()))
    }
    /// Whether `capture_target` can name a target at all here.
    fn can_capture(&self) -> bool {
        true
    }
    /// Linux only: tools the paste needs that aren't installed.
    fn missing_tools(&self) -> Option<MissingTools> {
        None
    }
}

/// Where text goes (Settings › Dictate › Where text goes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Into the focused app.
    Paste,
    /// "Copy to clipboard": never typed.
    Clipboard,
}

impl Output {
    pub fn parse(s: &str) -> Self {
        if s == "clipboard" {
            Self::Clipboard
        } else {
            Self::Paste
        }
    }
}

/// Why the text was left on the clipboard instead of pasted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum CopyReason {
    /// "Copy to clipboard" is the chosen output.
    Chosen,
    /// No target was captured, or it's gone.
    NoTarget,
    /// Focus couldn't be given back (another window took it).
    Focus,
    /// The keystroke couldn't be sent.
    Paste,
    /// Windows: the app runs as administrator.
    Elevated { app: String },
    /// The take was replaced or cancelled before it could paste.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pasted,
    Copied(CopyReason),
    /// Nothing was put anywhere (stale take, clipboard failure).
    Dropped,
}

/// The clipboard to put back after a paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRestore {
    pub previous: Clip,
    /// What we pasted: only restored while the clipboard still holds it.
    pub pasted: String,
}

/// Inserts `text` for a take. `still_current` is asked right before the
/// keystroke: a take replaced meanwhile never pastes.
pub fn insert(
    platform: &dyn Platform,
    target: Option<&Target>,
    text: &str,
    output: Output,
    still_current: &dyn Fn() -> bool,
) -> (Outcome, Option<PendingRestore>) {
    let terminal = target.is_some_and(|t| t.terminal);
    let text = if terminal {
        postproc::for_terminal(text)
    } else {
        text.to_string()
    };
    if !still_current() {
        return (Outcome::Dropped, None);
    }
    let copy = |reason: CopyReason| -> (Outcome, Option<PendingRestore>) {
        match platform.write_clipboard(&text) {
            Ok(()) => (Outcome::Copied(reason), None),
            Err(err) => {
                tracing::warn!(error = %err, "dictate: clipboard write failed");
                (Outcome::Dropped, None)
            }
        }
    };
    if output == Output::Clipboard {
        return copy(CopyReason::Chosen);
    }
    let target = match target {
        Some(t) if t.own => return copy(CopyReason::NoTarget),
        Some(t) if t.elevated => {
            return copy(CopyReason::Elevated {
                app: t.app_name.clone().unwrap_or_else(|| "This app".into()),
            })
        }
        Some(t) => Some(t),
        None if platform.can_capture() => return copy(CopyReason::NoTarget),
        // Wayland: nothing to verify against; paste into what has focus.
        None => None,
    };
    let previous = if platform.restores_clipboard() {
        platform.read_clipboard().ok()
    } else {
        None
    };
    if let Err(err) = platform.write_clipboard(&text) {
        tracing::warn!(error = %err, "dictate: clipboard write failed");
        return (Outcome::Dropped, None);
    }
    if let Some(t) = target {
        if platform.restore_focus(t).is_err() {
            return (Outcome::Copied(CopyReason::Focus), None);
        }
    }
    if !still_current() {
        // Replaced at the last moment: leave our text where it is, unpasted.
        return (Outcome::Copied(CopyReason::Stale), None);
    }
    if let Err(err) = platform.paste(terminal) {
        tracing::info!(error = %err, "dictate: paste failed; text left on the clipboard");
        return (Outcome::Copied(CopyReason::Paste), None);
    }
    let restore = previous.map(|previous| PendingRestore {
        previous,
        pasted: text.clone(),
    });
    (Outcome::Pasted, restore)
}

/// Puts the previous clipboard back, unless something else was copied since.
pub fn restore(platform: &dyn Platform, pending: &PendingRestore) {
    let still_ours = matches!(platform.read_clipboard(), Ok(Clip::Text(t)) if t == pending.pasted);
    if !still_ours {
        return;
    }
    if let Err(err) = platform.restore_clipboard(&pending.previous) {
        tracing::info!(error = %err, "dictate: clipboard not restored");
    }
}

/// Marks Tandem's own synthetic keystrokes (macOS event source data,
/// Windows `dwExtraInfo`), so the hotkey listeners ignore them.
pub const SYNTHETIC_MARK: i64 = 0x7a6e_646d;

/// Presses `modifiers`, taps `key`, releases in reverse, through enigo.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn chord(modifiers: &[enigo::Key], key: enigo::Key) -> Result<(), InsertError> {
    use enigo::{Direction, Enigo, Keyboard, Settings};
    let settings = Settings {
        event_source_user_data: Some(SYNTHETIC_MARK),
        windows_dw_extra_info: Some(SYNTHETIC_MARK as usize),
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut e = Enigo::new(&settings).map_err(|err| InsertError::Paste(err.to_string()))?;
    let err = |x: enigo::InputError| InsertError::Paste(x.to_string());
    for m in modifiers {
        e.key(*m, Direction::Press).map_err(err)?;
    }
    let tapped = e.key(key, Direction::Click).map_err(err);
    for m in modifiers.iter().rev() {
        let _ = e.key(*m, Direction::Release);
    }
    tapped
}

/// Reads and writes the clipboard through arboard, kept open for the whole
/// run (on X11 the owner must stay alive to serve what it copied).
pub(crate) struct Board(parking_lot::Mutex<Option<arboard::Clipboard>>);

impl Board {
    pub(crate) fn new() -> Self {
        Self(parking_lot::Mutex::new(None))
    }

    fn with<T>(
        &self,
        f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
    ) -> Result<T, InsertError> {
        let mut guard = self.0.lock();
        if guard.is_none() {
            *guard =
                Some(arboard::Clipboard::new().map_err(|e| InsertError::Clipboard(e.to_string()))?);
        }
        let board = guard
            .as_mut()
            .ok_or_else(|| InsertError::Clipboard("unavailable".into()))?;
        f(board).map_err(|e| InsertError::Clipboard(e.to_string()))
    }

    pub(crate) fn read(&self) -> Result<Clip, InsertError> {
        if let Ok(text) = self.with(|b| b.get_text()) {
            return Ok(Clip::Text(text));
        }
        match self.with(|b| b.get_image()) {
            Ok(img) => Ok(Clip::Image {
                width: img.width,
                height: img.height,
                rgba: img.bytes.into_owned(),
            }),
            Err(_) => Ok(Clip::Empty),
        }
    }

    pub(crate) fn write(&self, text: &str) -> Result<(), InsertError> {
        self.with(|b| b.set_text(text.to_string()))
    }

    pub(crate) fn restore(&self, clip: &Clip) -> Result<(), InsertError> {
        match clip {
            Clip::Empty => self.with(|b| b.clear()),
            Clip::Text(t) => self.write(t),
            Clip::Image {
                width,
                height,
                rgba,
            } => self.with(|b| {
                b.set_image(arboard::ImageData {
                    width: *width,
                    height: *height,
                    bytes: std::borrow::Cow::Borrowed(rgba),
                })
            }),
        }
    }
}

/// The platform for this OS.
pub fn native() -> std::sync::Arc<dyn Platform> {
    #[cfg(target_os = "linux")]
    {
        std::sync::Arc::new(linux::LinuxPlatform::new())
    }
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos::MacPlatform::new())
    }
    #[cfg(windows)]
    {
        std::sync::Arc::new(windows::WinPlatform::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        std::sync::Arc::new(fake::FakePlatform::default())
    }
}

/// Known terminal apps by bundle id, executable or WM class.
pub fn is_terminal_app(id: &str) -> bool {
    const TERMINALS: &[&str] = &[
        "com.apple.terminal",
        "com.googlecode.iterm2",
        "dev.warp.warp-stable",
        "net.kovidgoyal.kitty",
        "io.alacritty",
        "org.alacritty",
        "com.github.wez.wezterm",
        "co.zeit.hyper",
        "com.mitchellh.ghostty",
        "windowsterminal.exe",
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
        "conhost.exe",
        "wezterm-gui.exe",
        "alacritty.exe",
        "mintty.exe",
        "gnome-terminal",
        "gnome-terminal-server",
        "org.gnome.terminal",
        "org.gnome.console",
        "kgx",
        "konsole",
        "xterm",
        "uxterm",
        "urxvt",
        "rxvt",
        "kitty",
        "alacritty",
        "foot",
        "terminator",
        "tilix",
        "xfce4-terminal",
        "lxterminal",
        "mate-terminal",
        "st",
        "st-256color",
        "wezterm",
        "ghostty",
        "com.mitchellh.ghostty",
        "terminology",
        "qterminal",
    ];
    let id = id.to_lowercase();
    TERMINALS.contains(&id.as_str())
}

#[cfg(test)]
mod tests {
    use super::fake::{Call, FakePlatform};
    use super::*;

    fn target(terminal: bool) -> Target {
        Target {
            app_id: Some("org.gnome.gedit".into()),
            app_name: Some("Text Editor".into()),
            window: 42,
            element: 0,
            terminal,
            elevated: false,
            own: false,
        }
    }

    #[test]
    fn f_di_06_3_pastes_then_restores_the_previous_clipboard() {
        let p = FakePlatform::default();
        p.set_clipboard(Clip::Text("secret".into()));
        let (out, pending) = insert(
            &p,
            Some(&target(false)),
            "hello world",
            Output::Paste,
            &|| true,
        );
        assert_eq!(out, Outcome::Pasted);
        assert_eq!(p.pasted(), vec!["hello world".to_string()]);
        restore(&p, &pending.expect("restore"));
        assert_eq!(p.clipboard(), Clip::Text("secret".into()));
        assert_eq!(
            p.calls(),
            vec![
                Call::ReadClipboard,
                Call::WriteClipboard("hello world".into()),
                Call::RestoreFocus(42),
                Call::Paste { terminal: false },
                Call::ReadClipboard,
                Call::RestoreClipboard,
            ]
        );
    }

    #[test]
    fn f_di_06_windows_never_restores_the_clipboard() {
        let p = FakePlatform::default();
        p.set_restores(false);
        let (out, pending) = insert(&p, Some(&target(false)), "hi", Output::Paste, &|| true);
        assert_eq!(out, Outcome::Pasted);
        assert!(pending.is_none());
        assert!(!p.calls().contains(&Call::ReadClipboard));
    }

    #[test]
    fn f_di_06_2_focus_failure_leaves_the_text_on_the_clipboard() {
        let p = FakePlatform::default();
        p.fail_focus(true);
        let (out, pending) = insert(&p, Some(&target(false)), "hello", Output::Paste, &|| true);
        assert_eq!(out, Outcome::Copied(CopyReason::Focus));
        assert!(pending.is_none());
        assert_eq!(p.clipboard(), Clip::Text("hello".into()));
        assert!(p.pasted().is_empty());
    }

    #[test]
    fn f_di_06_paste_failure_and_lost_target_copy_instead() {
        let p = FakePlatform::default();
        p.fail_paste(true);
        let (out, _) = insert(&p, Some(&target(false)), "x", Output::Paste, &|| true);
        assert_eq!(out, Outcome::Copied(CopyReason::Paste));
        assert_eq!(p.clipboard(), Clip::Text("x".into()));
        let p = FakePlatform::default();
        let (out, _) = insert(&p, None, "y", Output::Paste, &|| true);
        assert_eq!(out, Outcome::Copied(CopyReason::NoTarget));
        assert!(p.pasted().is_empty());
    }

    #[test]
    fn f_di_06_4_terminals_get_no_control_chars_and_no_cr() {
        let p = FakePlatform::default();
        let (out, _) = insert(
            &p,
            Some(&target(true)),
            "ls\u{1b}[2J\r\n",
            Output::Paste,
            &|| true,
        );
        assert_eq!(out, Outcome::Pasted);
        assert_eq!(p.pasted(), vec!["ls2J".to_string()]);
        assert!(p.calls().contains(&Call::Paste { terminal: true }));
    }

    #[test]
    fn f_di_06_stale_takes_never_paste() {
        let p = FakePlatform::default();
        let (out, _) = insert(&p, Some(&target(false)), "late", Output::Paste, &|| false);
        assert_eq!(out, Outcome::Dropped);
        assert!(p.calls().is_empty(), "{:?}", p.calls());
        // Replaced between focus and keystroke: copied, not pasted.
        let p = FakePlatform::default();
        let n = std::sync::atomic::AtomicUsize::new(0);
        let current = || n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        let (out, _) = insert(&p, Some(&target(false)), "late", Output::Paste, &current);
        assert_eq!(out, Outcome::Copied(CopyReason::Stale));
        assert!(p.pasted().is_empty());
    }

    #[test]
    fn f_di_06_own_elevated_and_copy_mode() {
        let p = FakePlatform::default();
        let own = Target {
            own: true,
            ..target(false)
        };
        assert_eq!(
            insert(&p, Some(&own), "a", Output::Paste, &|| true).0,
            Outcome::Copied(CopyReason::NoTarget)
        );
        let admin = Target {
            elevated: true,
            app_name: Some("Registry Editor".into()),
            ..target(false)
        };
        assert_eq!(
            insert(&p, Some(&admin), "a", Output::Paste, &|| true).0,
            Outcome::Copied(CopyReason::Elevated {
                app: "Registry Editor".into()
            })
        );
        assert_eq!(
            insert(&p, Some(&target(false)), "a", Output::Clipboard, &|| true).0,
            Outcome::Copied(CopyReason::Chosen)
        );
        assert!(p.pasted().is_empty());
    }

    #[test]
    fn f_di_06_wayland_pastes_without_a_target() {
        let p = FakePlatform::default();
        p.set_can_capture(false);
        let (out, _) = insert(&p, None, "hi", Output::Paste, &|| true);
        assert_eq!(out, Outcome::Pasted);
        assert!(!p.calls().iter().any(|c| matches!(c, Call::RestoreFocus(_))));
    }

    #[test]
    fn f_di_06_restore_skips_when_the_user_copied_something_else() {
        let p = FakePlatform::default();
        p.set_clipboard(Clip::Text("old".into()));
        let (_, pending) = insert(&p, Some(&target(false)), "new", Output::Paste, &|| true);
        p.set_clipboard(Clip::Text("user copied this".into()));
        restore(&p, &pending.expect("pending"));
        assert_eq!(p.clipboard(), Clip::Text("user copied this".into()));
    }

    #[test]
    fn terminals_and_missing_tools() {
        assert!(is_terminal_app("com.googlecode.iterm2"));
        assert!(is_terminal_app("WindowsTerminal.exe"));
        assert!(!is_terminal_app("org.gnome.gedit"));
        let m = MissingTools::new(&["wtype", "wl-clipboard"]);
        assert_eq!(m.apt, "sudo apt install wtype wl-clipboard");
        assert_eq!(m.pacman, "sudo pacman -S wtype wl-clipboard");
    }
}
