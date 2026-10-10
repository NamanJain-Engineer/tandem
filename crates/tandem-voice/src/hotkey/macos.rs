//! macOS hotkeys (F-DI-02): a listen-only `CGEventTap` on a thread with its
//! own run loop. It sees Fn through the flags, re-enables itself when the
//! system disables it, pauses while secure input is on, and needs Input
//! Monitoring.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::binding::Modifier;
use super::keymaps::{mac_flags_to_modifiers, mac_fn_flag_is_real, mac_keycode_to_raw};
use super::{
    now_ms, Bindings, HotkeyBackend, HotkeyError, HotkeyStatus, KeyEvent, KeyState, RawKey,
};

type CFTypeRef = *const c_void;
type CFMachPortRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;

type TapCallback =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: CFTypeRef;
    fn CFMachPortCreateRunLoopSource(
        alloc: CFTypeRef,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFTypeRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFRelease(cf: CFTypeRef);
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
}

const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const LISTEN_ONLY: u32 = 1;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const OTHER_MOUSE_DOWN: u32 = 25;
const OTHER_MOUSE_UP: u32 = 26;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const FIELD_KEYCODE: u32 = 9;
const FIELD_BUTTON: u32 = 3;
const FIELD_SOURCE_PID: u32 = 41;
const LISTEN_EVENT: u32 = 1;

/// Input Monitoring: granted (0), denied (1), not asked yet (2).
pub fn input_monitoring_granted() -> bool {
    // SAFETY: a plain IOKit query.
    unsafe { IOHIDCheckAccess(LISTEN_EVENT) == 0 }
}

pub fn secure_input_on() -> bool {
    // SAFETY: a plain Carbon query.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

struct TapState {
    keys: KeyState,
    held: HashSet<RawKey>,
    tx: mpsc::Sender<KeyEvent>,
    esc: Arc<AtomicBool>,
    port: AtomicPtr<c_void>,
    own_pid: i64,
}

unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    kind: u32,
    event: CGEventRef,
    user: *mut c_void,
) -> CGEventRef {
    // SAFETY: `user` is the `Mutex<TapState>` the tap thread owns for the
    // tap's whole life.
    let state = unsafe { &*(user as *const Mutex<TapState>) };
    let Ok(mut s) = state.lock() else {
        return event;
    };
    if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER_INPUT {
        // Self-heal: the system turned the tap off; turn it back on.
        let port = s.port.load(Ordering::Acquire);
        if !port.is_null() {
            // SAFETY: the port is alive while the run loop runs.
            unsafe { CGEventTapEnable(port, true) };
        }
        return event;
    }
    // SAFETY: `event` is valid for the callback's duration.
    let pid = unsafe { CGEventGetIntegerValueField(event, FIELD_SOURCE_PID) };
    if pid == s.own_pid {
        return event; // our own paste
    }
    let esc = s.esc.load(Ordering::Acquire);
    let mut transitions: Vec<(RawKey, bool)> = Vec::new();
    match kind {
        FLAGS_CHANGED => {
            // SAFETY: as above.
            let flags = unsafe { CGEventGetFlags(event) };
            let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) } as u16;
            let now: HashSet<Modifier> = mac_flags_to_modifiers(flags)
                .into_iter()
                .filter(|m| *m != Modifier::Fn || mac_fn_flag_is_real(code))
                .collect();
            for m in [
                Modifier::Ctrl,
                Modifier::Alt,
                Modifier::Shift,
                Modifier::Meta,
                Modifier::Fn,
            ] {
                let key = RawKey::Modifier(m);
                let was = s.held.contains(&key);
                let is = now.contains(&m);
                if was != is {
                    transitions.push((key, is));
                }
            }
        }
        KEY_DOWN | KEY_UP => {
            let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) } as u16;
            if let Some(key) = mac_keycode_to_raw(code) {
                let down = kind == KEY_DOWN;
                if s.held.contains(&key) != down {
                    transitions.push((key, down));
                }
            }
        }
        OTHER_MOUSE_DOWN | OTHER_MOUSE_UP => {
            let button = unsafe { CGEventGetIntegerValueField(event, FIELD_BUTTON) };
            let key = match button {
                3 => Some(RawKey::Mouse4),
                4 => Some(RawKey::Mouse5),
                _ => None,
            };
            if let Some(key) = key {
                transitions.push((key, kind == OTHER_MOUSE_DOWN));
            }
        }
        _ => {}
    }
    for (key, down) in transitions {
        if down {
            s.held.insert(key.clone());
        } else {
            s.held.remove(&key);
        }
        let out = s.keys.transition(key, down, esc, now_ms());
        for e in out {
            let _ = s.tx.send(e);
        }
    }
    event
}

/// `*mut c_void` that may cross to the tap thread.
struct SendPtr(*mut c_void);
// SAFETY: only the run loop reference is sent, to be stopped from another
// thread with CFRunLoopStop, which is thread-safe.
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

pub struct MacEventTap {
    run_loop: Mutex<Option<SendPtr>>,
    esc: Arc<AtomicBool>,
    status: Arc<Mutex<HotkeyStatus>>,
    alive: Arc<AtomicBool>,
}

impl MacEventTap {
    pub fn new() -> Self {
        Self {
            run_loop: Mutex::new(None),
            esc: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(HotkeyStatus::NotRunning)),
            alive: Arc::new(AtomicBool::new(false)),
        }
    }

    fn set(&self, s: HotkeyStatus) {
        if let Ok(mut g) = self.status.lock() {
            *g = s;
        }
    }
}

impl Default for MacEventTap {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyBackend for MacEventTap {
    fn start(
        &self,
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
    ) -> Result<HotkeyStatus, HotkeyError> {
        self.stop();
        if !input_monitoring_granted() {
            self.set(HotkeyStatus::NeedsPermission);
            return Err(HotkeyError::NeedsPermission);
        }
        let state = Box::new(Mutex::new(TapState {
            keys: KeyState::new(bindings),
            held: HashSet::new(),
            tx,
            esc: self.esc.clone(),
            port: AtomicPtr::new(std::ptr::null_mut()),
            own_pid: std::process::id() as i64,
        }));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<SendPtr, String>>();
        self.alive.store(true, Ordering::Release);
        let alive = self.alive.clone();
        std::thread::Builder::new()
            .name("dictate-hotkey-mac".into())
            .spawn(move || {
                let mask: u64 = (1 << KEY_DOWN)
                    | (1 << KEY_UP)
                    | (1 << FLAGS_CHANGED)
                    | (1 << OTHER_MOUSE_DOWN)
                    | (1 << OTHER_MOUSE_UP);
                let user = &*state as *const Mutex<TapState> as *mut c_void;
                // SAFETY: CoreGraphics/CoreFoundation calls on this thread;
                // `state` outlives the run loop, which ends before it drops.
                unsafe {
                    let port = CGEventTapCreate(
                        SESSION_TAP,
                        HEAD_INSERT,
                        LISTEN_ONLY,
                        mask,
                        tap_callback,
                        user,
                    );
                    if port.is_null() {
                        let _ = ready_tx.send(Err("the event tap couldn't be created".into()));
                        return;
                    }
                    if let Ok(s) = state.lock() {
                        s.port.store(port, Ordering::Release);
                    }
                    let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                    let rl = CFRunLoopGetCurrent();
                    CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
                    CGEventTapEnable(port, true);
                    let _ = ready_tx.send(Ok(SendPtr(rl)));
                    CFRunLoopRun();
                    CGEventTapEnable(port, false);
                    CFRelease(source as CFTypeRef);
                    CFRelease(port as CFTypeRef);
                }
                alive.store(false, Ordering::Release);
                drop(state);
            })
            .map_err(|e| HotkeyError::Failed(e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(rl)) => {
                if let Ok(mut g) = self.run_loop.lock() {
                    *g = Some(rl);
                }
                // Secure input pauses Dictate: polled once a second.
                let (status, alive) = (self.status.clone(), self.alive.clone());
                std::thread::spawn(move || {
                    while alive.load(Ordering::Acquire) {
                        let paused = secure_input_on();
                        if let Ok(mut g) = status.lock() {
                            match (&*g, paused) {
                                (HotkeyStatus::Active, true) => *g = HotkeyStatus::Paused,
                                (HotkeyStatus::Paused, false) => *g = HotkeyStatus::Active,
                                _ => {}
                            }
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                });
                self.set(HotkeyStatus::Active);
                Ok(HotkeyStatus::Active)
            }
            Ok(Err(e)) => {
                self.set(HotkeyStatus::NotRunning);
                Err(HotkeyError::Failed(e))
            }
            Err(_) => Err(HotkeyError::Failed("the event tap didn't start".into())),
        }
    }

    fn stop(&self) {
        if let Ok(mut g) = self.run_loop.lock() {
            if let Some(rl) = g.take() {
                // SAFETY: stopping a live run loop from another thread is allowed.
                unsafe { CFRunLoopStop(rl.0) };
            }
        }
        self.alive.store(false, Ordering::Release);
        self.set(HotkeyStatus::NotRunning);
    }

    fn listen_esc(&self, on: bool) {
        self.esc.store(on, Ordering::Release);
    }

    fn status(&self) -> HotkeyStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(HotkeyStatus::NotRunning)
    }
}
