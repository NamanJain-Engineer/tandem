//! Linux insertion (F-DI-06). X11: the active window is captured from
//! `_NET_ACTIVE_WINDOW`, focus is given back through the window manager and
//! read back, and the paste is a synthetic Ctrl+V (Ctrl+Shift+V in a
//! terminal) through enigo's XTest backend. Wayland: no app can name or
//! focus another window, so the paste goes to whatever has focus, through
//! `wtype`, and the clipboard through arboard's data-control backend or
//! `wl-copy`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window,
};
use x11rb::rust_connection::RustConnection;

use super::{is_terminal_app, Board, Clip, InsertError, MissingTools, Platform, Target};
use crate::hotkey::linux::Session;

pub struct LinuxPlatform {
    session: Session,
    board: Board,
}

impl LinuxPlatform {
    pub fn new() -> Self {
        Self {
            session: Session::detect(),
            board: Board::new(),
        }
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let p = dir.join(tool);
            p.is_file()
        })
    })
}

struct X {
    conn: RustConnection,
    root: Window,
}

impl X {
    fn connect() -> Result<Self, InsertError> {
        let (conn, screen) =
            x11rb::connect(None).map_err(|e| InsertError::Unsupported(format!("X11: {e}")))?;
        let root = conn.setup().roots[screen].root;
        Ok(Self { conn, root })
    }

    fn atom(&self, name: &str) -> Result<u32, InsertError> {
        Ok(self
            .conn
            .intern_atom(false, name.as_bytes())
            .map_err(|_| InsertError::TargetGone)?
            .reply()
            .map_err(|_| InsertError::TargetGone)?
            .atom)
    }

    fn active(&self) -> Result<Option<Window>, InsertError> {
        let atom = self.atom("_NET_ACTIVE_WINDOW")?;
        let reply = self
            .conn
            .get_property(false, self.root, atom, AtomEnum::WINDOW, 0, 1)
            .map_err(|_| InsertError::TargetGone)?
            .reply()
            .map_err(|_| InsertError::TargetGone)?;
        Ok(reply
            .value32()
            .and_then(|mut v| v.next())
            .filter(|w| *w != 0))
    }

    fn class(&self, w: Window) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        // "instance\0Class\0": the class names the app.
        let parts: Vec<&[u8]> = reply
            .value
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .collect();
        parts.last().map(|p| String::from_utf8_lossy(p).to_string())
    }

    fn pid(&self, w: Window) -> Option<u32> {
        let atom = self.atom("_NET_WM_PID").ok()?;
        let reply = self
            .conn
            .get_property(false, w, atom, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        reply.value32().and_then(|mut v| v.next())
    }

    fn exists(&self, w: Window) -> bool {
        self.conn
            .get_window_attributes(w)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some()
    }

    fn activate(&self, w: Window) -> Result<(), InsertError> {
        let atom = self.atom("_NET_ACTIVE_WINDOW")?;
        // Source 2: a pager-like request, which window managers honour
        // without focus-stealing prevention.
        let event = ClientMessageEvent::new(32, w, atom, [2u32, 0, 0, 0, 0]);
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|_| InsertError::Focus)?;
        self.conn.flush().map_err(|_| InsertError::Focus)?;
        Ok(())
    }
}

impl Platform for LinuxPlatform {
    fn capture_target(&self) -> Result<Option<Target>, InsertError> {
        if self.session != Session::X11 {
            return Ok(None);
        }
        let x = X::connect()?;
        let Some(w) = x.active()? else {
            return Ok(None);
        };
        let class = x.class(w);
        let own = x.pid(w) == Some(std::process::id());
        Ok(Some(Target {
            terminal: class.as_deref().is_some_and(is_terminal_app),
            app_name: class.clone(),
            app_id: class,
            window: w as u64,
            element: 0,
            elevated: false,
            own,
        }))
    }

    fn restore_focus(&self, target: &Target) -> Result<(), InsertError> {
        let x = X::connect()?;
        let w = target.window as Window;
        if !x.exists(w) {
            return Err(InsertError::TargetGone);
        }
        if x.active()? == Some(w) {
            return Ok(());
        }
        x.activate(w)?;
        // Confirm by reading the active window back.
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            if x.active()? == Some(w) {
                // Give the app a moment to take keyboard focus.
                std::thread::sleep(Duration::from_millis(30));
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(InsertError::Focus)
    }

    fn paste(&self, terminal: bool) -> Result<(), InsertError> {
        if self.session == Session::Wayland {
            if !on_path("wtype") {
                return Err(InsertError::Paste("wtype isn't installed".into()));
            }
            let mut args = vec!["-M", "ctrl"];
            if terminal {
                args.extend(["-M", "shift"]);
            }
            args.extend(["-k", "v"]);
            if terminal {
                args.extend(["-m", "shift"]);
            }
            args.extend(["-m", "ctrl"]);
            let ok = Command::new("wtype")
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            return if ok {
                Ok(())
            } else {
                Err(InsertError::Paste("wtype failed".into()))
            };
        }
        use enigo::Key;
        let mods: &[Key] = if terminal {
            &[Key::Control, Key::Shift]
        } else {
            &[Key::Control]
        };
        super::chord(mods, Key::Unicode('v'))
    }

    fn undo(&self) -> Result<(), InsertError> {
        if self.session == Session::Wayland {
            let ok = on_path("wtype")
                && Command::new("wtype")
                    .args(["-M", "ctrl", "-k", "z", "-m", "ctrl"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
            return if ok {
                Ok(())
            } else {
                Err(InsertError::Paste("wtype failed".into()))
            };
        }
        super::chord(&[enigo::Key::Control], enigo::Key::Unicode('z'))
    }

    fn read_clipboard(&self) -> Result<Clip, InsertError> {
        match self.board.read() {
            Ok(c) => Ok(c),
            Err(_) if self.session == Session::Wayland && on_path("wl-paste") => {
                let out = Command::new("wl-paste")
                    .arg("--no-newline")
                    .output()
                    .map_err(|e| InsertError::Clipboard(e.to_string()))?;
                Ok(if out.status.success() {
                    Clip::Text(String::from_utf8_lossy(&out.stdout).to_string())
                } else {
                    Clip::Empty
                })
            }
            Err(e) => Err(e),
        }
    }

    fn write_clipboard(&self, text: &str) -> Result<(), InsertError> {
        match self.board.write(text) {
            Ok(()) => Ok(()),
            Err(_) if self.session == Session::Wayland && on_path("wl-copy") => {
                use std::io::Write;
                let mut child = Command::new("wl-copy")
                    .stdin(Stdio::piped())
                    .spawn()
                    .map_err(|e| InsertError::Clipboard(e.to_string()))?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin
                        .write_all(text.as_bytes())
                        .map_err(|e| InsertError::Clipboard(e.to_string()))?;
                }
                child
                    .wait()
                    .map_err(|e| InsertError::Clipboard(e.to_string()))?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn restore_clipboard(&self, clip: &Clip) -> Result<(), InsertError> {
        match clip {
            Clip::Text(t) => self.write_clipboard(t),
            other => self.board.restore(other),
        }
    }

    fn restores_clipboard(&self) -> bool {
        true
    }

    fn can_capture(&self) -> bool {
        self.session == Session::X11
    }

    fn missing_tools(&self) -> Option<MissingTools> {
        match self.session {
            Session::Wayland => {
                let mut missing = Vec::new();
                if !on_path("wtype") {
                    missing.push("wtype");
                }
                if !on_path("wl-copy") {
                    missing.push("wl-clipboard");
                }
                (!missing.is_empty()).then(|| MissingTools::new(&missing))
            }
            _ => None,
        }
    }
}
