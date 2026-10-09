//! Linux hotkeys (F-DI-02, ARCHITECTURE §12.2): XInput2 raw key events on
//! X11, the XDG GlobalShortcuts portal on Wayland. Both only listen; neither
//! grabs a key away from the focused app.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::Mutex;

use super::binding::{Binding, Modifier, Trigger};
use super::keymaps::keysym_to_raw;
use super::RawKey;
use super::{
    now_ms, Bindings, HotkeyBackend, HotkeyError, HotkeyStatus, KeyEvent, KeyState, Phase, Which,
};

/// What kind of desktop session this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
    /// Neither display is reachable (a TTY, a headless box).
    None,
}

impl Session {
    /// From the environment the app was started with.
    pub fn detect() -> Self {
        Self::from_env(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
            std::env::var_os("DISPLAY").is_some(),
        )
    }

    pub fn from_env(session_type: Option<&str>, wayland: bool, x11: bool) -> Self {
        match session_type {
            Some("wayland") => Self::Wayland,
            Some("x11") if x11 => Self::X11,
            _ if wayland => Self::Wayland,
            _ if x11 => Self::X11,
            _ => Self::None,
        }
    }
}

/// The fallback the UI shows when no global shortcut can be registered.
pub const DEEP_LINK_COMMAND: &str = "xdg-open tandem://dictate/toggle";

pub struct LinuxHotkeys {
    session: Session,
    status: Arc<Mutex<HotkeyStatus>>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
    esc: Arc<AtomicBool>,
}

impl LinuxHotkeys {
    pub fn new() -> Self {
        Self::for_session(Session::detect())
    }

    pub fn for_session(session: Session) -> Self {
        Self {
            session,
            status: Arc::new(Mutex::new(HotkeyStatus::NotRunning)),
            stop: Mutex::new(None),
            esc: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for LinuxHotkeys {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyBackend for LinuxHotkeys {
    fn start(
        &self,
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
    ) -> Result<HotkeyStatus, HotkeyError> {
        self.stop();
        let stop = Arc::new(AtomicBool::new(false));
        *self.stop.lock() = Some(stop.clone());
        *self.status.lock() = HotkeyStatus::Starting;
        let result = match self.session {
            Session::X11 => x11::start(bindings, tx, stop, self.esc.clone()),
            Session::Wayland => portal::start(bindings, tx, stop, self.status.clone()),
            Session::None => Err(HotkeyError::Unavailable("no display".into())),
        };
        let status = match &result {
            Ok(s) => s.clone(),
            Err(HotkeyError::Unavailable(reason)) => HotkeyStatus::Unavailable {
                reason: reason.clone(),
            },
            Err(HotkeyError::NeedsPermission) => HotkeyStatus::NeedsPermission,
            Err(HotkeyError::Failed(_)) => HotkeyStatus::NotRunning,
        };
        *self.status.lock() = status;
        result
    }

    fn stop(&self) {
        if let Some(stop) = self.stop.lock().take() {
            stop.store(true, Ordering::Release);
        }
        *self.status.lock() = HotkeyStatus::NotRunning;
    }

    fn listen_esc(&self, on: bool) {
        self.esc.store(on, Ordering::Release);
    }

    fn status(&self) -> HotkeyStatus {
        self.status.lock().clone()
    }
}

mod x11 {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::xinput::{self, ConnectionExt as _, XIEventMask};
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::protocol::Event;

    pub fn start(
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
        stop: Arc<AtomicBool>,
        esc: Arc<AtomicBool>,
    ) -> Result<HotkeyStatus, HotkeyError> {
        let (conn, screen) =
            x11rb::connect(None).map_err(|e| HotkeyError::Unavailable(format!("X11: {e}")))?;
        let root = conn.setup().roots[screen].root;
        let version = conn
            .xinput_xi_query_version(2, 0)
            .map_err(|e| HotkeyError::Failed(e.to_string()))?
            .reply()
            .map_err(|_| HotkeyError::Unavailable("XInput2 isn't available".into()))?;
        if version.major_version < 2 {
            return Err(HotkeyError::Unavailable("XInput2 isn't available".into()));
        }
        let mask = XIEventMask::RAW_KEY_PRESS
            | XIEventMask::RAW_KEY_RELEASE
            | XIEventMask::RAW_BUTTON_PRESS
            | XIEventMask::RAW_BUTTON_RELEASE;
        conn.xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: xinput::Device::ALL_MASTER.into(),
                mask: vec![mask],
            }],
        )
        .map_err(|e| HotkeyError::Failed(e.to_string()))?
        .check()
        .map_err(|e| HotkeyError::Failed(e.to_string()))?;
        let keymap = keymap(&conn).map_err(HotkeyError::Failed)?;
        let mut state = KeyState::new(bindings);
        std::thread::Builder::new()
            .name("dictate-hotkey-x11".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let event = match conn.poll_for_event() {
                        Ok(Some(e)) => e,
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(4));
                            continue;
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "dictate: X11 connection lost");
                            break;
                        }
                    };
                    let (key, down) = match event {
                        Event::XinputRawKeyPress(e) => {
                            (keymap.get(&(e.detail as u8)).cloned(), true)
                        }
                        Event::XinputRawKeyRelease(e) => {
                            (keymap.get(&(e.detail as u8)).cloned(), false)
                        }
                        Event::XinputRawButtonPress(e) => (button(e.detail), true),
                        Event::XinputRawButtonRelease(e) => (button(e.detail), false),
                        _ => (None, false),
                    };
                    let Some(key) = key else { continue };
                    for e in state.transition(key, down, esc.load(Ordering::Acquire), now_ms()) {
                        if tx.send(e).is_err() {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| HotkeyError::Failed(e.to_string()))?;
        Ok(HotkeyStatus::Active)
    }

    fn button(detail: u32) -> Option<RawKey> {
        match detail {
            8 => Some(RawKey::Mouse4),
            9 => Some(RawKey::Mouse5),
            _ => None,
        }
    }

    /// Keycode → the key a binding names, from the server's keyboard map
    /// (first keysym per code).
    fn keymap(conn: &impl Connection) -> Result<HashMap<u8, RawKey>, String> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let per = map.keysyms_per_keycode.max(1) as usize;
        let mut out = HashMap::new();
        for (i, syms) in map.keysyms.chunks(per).enumerate() {
            let code = min as usize + i;
            if let Some(raw) = syms.iter().find_map(|s| keysym_to_raw(*s)) {
                out.insert(code as u8, raw);
            }
        }
        Ok(out)
    }
}

mod portal {
    use super::*;
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures::StreamExt;

    /// The portal's trigger syntax (`CTRL+ALT+d`): a hint the desktop may
    /// change when it asks the user to approve the shortcut.
    pub fn trigger(binding: &Binding) -> String {
        let mut parts: Vec<String> = binding
            .modifiers
            .iter()
            .filter_map(|m| match m {
                Modifier::Ctrl => Some("CTRL".to_string()),
                Modifier::Alt => Some("ALT".into()),
                Modifier::Shift => Some("SHIFT".into()),
                Modifier::Meta => Some("LOGO".into()),
                Modifier::Fn => None,
            })
            .collect();
        match &binding.trigger {
            Some(Trigger::Key(k)) if k.len() == 1 => parts.push(k.to_lowercase()),
            Some(Trigger::Key(k)) => parts.push(k.clone()),
            _ => {}
        }
        parts.join("+")
    }

    pub fn start(
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
        stop: Arc<AtomicBool>,
        status: Arc<Mutex<HotkeyStatus>>,
    ) -> Result<HotkeyStatus, HotkeyError> {
        let mut shortcuts = Vec::new();
        if let Some(b) = &bindings.hold {
            shortcuts.push(("dictate", "Hold to dictate", trigger(b), Which::Hold));
        }
        if let Some(b) = &bindings.toggle {
            shortcuts.push((
                "dictate-toggle",
                "Start or stop dictating",
                trigger(b),
                Which::Toggle,
            ));
        }
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), HotkeyError>>();
        std::thread::Builder::new()
            .name("dictate-hotkey-portal".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    let _ = ready_tx.send(Err(HotkeyError::Failed("runtime".into())));
                    return;
                };
                rt.block_on(async move {
                    let run = async {
                        let proxy = GlobalShortcuts::new()
                            .await
                            .map_err(|e| HotkeyError::Unavailable(format!("portal: {e}")))?;
                        let session = proxy
                            .create_session()
                            .await
                            .map_err(|e| HotkeyError::Unavailable(format!("portal: {e}")))?;
                        let new: Vec<NewShortcut> = shortcuts
                            .iter()
                            .map(|(id, desc, trig, _)| {
                                NewShortcut::new(*id, *desc).preferred_trigger(Some(trig.as_str()))
                            })
                            .collect();
                        proxy
                            .bind_shortcuts(&session, &new, None)
                            .await
                            .and_then(|r| r.response())
                            .map_err(|e| HotkeyError::Unavailable(format!("portal: {e}")))?;
                        let activated = proxy
                            .receive_activated()
                            .await
                            .map_err(|e| HotkeyError::Failed(e.to_string()))?;
                        let deactivated = proxy
                            .receive_deactivated()
                            .await
                            .map_err(|e| HotkeyError::Failed(e.to_string()))?;
                        Ok::<_, HotkeyError>((proxy, session, activated, deactivated))
                    };
                    let (_proxy, _session, activated, deactivated) = match run.await {
                        Ok(v) => {
                            let _ = ready_tx.send(Ok(()));
                            v
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    let which = |id: &str| {
                        shortcuts
                            .iter()
                            .find(|(sid, ..)| *sid == id)
                            .map(|(.., w)| *w)
                    };
                    let mut events = futures::stream::select(
                        activated
                            .map(|a| (a.shortcut_id().to_string(), Phase::Down))
                            .boxed(),
                        deactivated
                            .map(|d| (d.shortcut_id().to_string(), Phase::Up))
                            .boxed(),
                    );
                    loop {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let next =
                            tokio::time::timeout(Duration::from_millis(200), events.next()).await;
                        match next {
                            Ok(Some((id, phase))) => {
                                if let Some(w) = which(&id) {
                                    let e = KeyEvent {
                                        which: w,
                                        phase,
                                        native: true,
                                        at_ms: now_ms(),
                                    };
                                    if tx.send(e).is_err() {
                                        break;
                                    }
                                }
                            }
                            Ok(None) => {
                                *status.lock() = HotkeyStatus::NotRunning;
                                break;
                            }
                            Err(_) => {}
                        }
                    }
                });
            })
            .map_err(|e| HotkeyError::Failed(e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_secs(60)) {
            Ok(Ok(())) => Ok(HotkeyStatus::Active),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HotkeyError::Unavailable(
                "the desktop didn't answer the shortcut request".into(),
            )),
        }
    }
}

pub use portal::trigger as portal_trigger;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_02_session_detection() {
        assert_eq!(
            Session::from_env(Some("wayland"), true, true),
            Session::Wayland
        );
        assert_eq!(Session::from_env(Some("x11"), false, true), Session::X11);
        assert_eq!(Session::from_env(None, true, false), Session::Wayland);
        assert_eq!(Session::from_env(None, false, true), Session::X11);
        assert_eq!(Session::from_env(Some("tty"), false, false), Session::None);
    }

    #[test]
    fn f_di_02_3_wayland_without_the_portal_is_unavailable_with_the_fallback() {
        // No session bus in this test process: the portal can't be reached.
        let saved = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        std::env::set_var(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/tandem-test-bus",
        );
        let hk = LinuxHotkeys::for_session(Session::Wayland);
        let (tx, _rx) = mpsc::channel();
        let b = Bindings {
            hold: Some("Ctrl+Alt".parse().expect("b")),
            toggle: None,
        };
        let res = hk.start(&b, tx);
        match saved {
            Some(v) => std::env::set_var("DBUS_SESSION_BUS_ADDRESS", v),
            None => std::env::remove_var("DBUS_SESSION_BUS_ADDRESS"),
        }
        assert!(matches!(res, Err(HotkeyError::Unavailable(_))), "{res:?}");
        assert!(matches!(hk.status(), HotkeyStatus::Unavailable { .. }));
        assert_eq!(DEEP_LINK_COMMAND, "xdg-open tandem://dictate/toggle");
    }

    #[test]
    fn f_di_02_portal_trigger_syntax() {
        assert_eq!(portal_trigger(&"Ctrl+Alt".parse().expect("b")), "CTRL+ALT");
        assert_eq!(
            portal_trigger(&"Meta+Shift+D".parse().expect("b")),
            "SHIFT+LOGO+d"
        );
    }

    #[test]
    fn f_di_02_keysyms_and_key_state() {
        assert_eq!(
            keysym_to_raw(0xffe3),
            Some(RawKey::Modifier(Modifier::Ctrl))
        );
        assert_eq!(keysym_to_raw(0x64), Some(RawKey::Key("D".into())));
        assert_eq!(keysym_to_raw(0xffc9), Some(RawKey::Key("F12".into())));
        let b = Bindings {
            hold: Some("Ctrl+Alt".parse().expect("b")),
            toggle: Some("Ctrl+Alt+D".parse().expect("b")),
        };
        let mut s = KeyState::new(&b);
        let ctrl = RawKey::Modifier(Modifier::Ctrl);
        let alt = RawKey::Modifier(Modifier::Alt);
        assert!(s.transition(ctrl.clone(), true, false, 0).is_empty());
        let e = s.transition(alt.clone(), true, false, 1);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].which, e[0].phase), (Which::Hold, Phase::Down));
        let e = s.transition(RawKey::Key("D".into()), true, false, 2);
        assert_eq!((e[0].which, e[0].phase), (Which::Toggle, Phase::Down));
        let e = s.transition(RawKey::Key("Esc".into()), true, true, 3);
        assert_eq!(e[0].which, Which::Esc);
        let e = s.transition(alt, false, false, 4);
        assert!(e
            .iter()
            .any(|e| e.which == Which::Hold && e.phase == Phase::Up));
    }

    /// Real XInput2 under the test display: `xdotool` presses Ctrl+Alt and
    /// the listener reports Down then Up. Runs where DISPLAY and xdotool exist.
    #[test]
    fn f_di_02_x11_listener_hears_a_real_press_and_release() {
        if std::env::var_os("DISPLAY").is_none()
            || std::process::Command::new("xdotool")
                .arg("version")
                .output()
                .is_err()
        {
            eprintln!("skipped: no X display or xdotool");
            return;
        }
        let hk = LinuxHotkeys::for_session(Session::X11);
        let (tx, rx) = mpsc::channel();
        let b = Bindings {
            hold: Some("Ctrl+Alt".parse().expect("b")),
            toggle: None,
        };
        assert_eq!(hk.start(&b, tx), Ok(HotkeyStatus::Active));
        std::thread::sleep(Duration::from_millis(100));
        let run = |args: &[&str]| {
            let _ = std::process::Command::new("xdotool").args(args).status();
        };
        run(&["keydown", "Control_L"]);
        run(&["keydown", "Alt_L"]);
        std::thread::sleep(Duration::from_millis(50));
        run(&["keyup", "Alt_L"]);
        run(&["keyup", "Control_L"]);
        let mut got = Vec::new();
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(2)) {
            got.push(e.phase);
            if got.len() == 2 {
                break;
            }
        }
        hk.stop();
        assert_eq!(got, vec![Phase::Down, Phase::Up]);
    }
}
