//! Windows hotkeys (F-DI-02): a listen-only `WH_KEYBOARD_LL` hook (and
//! `WH_MOUSE_LL` for Mouse 4/5) on a thread with its own message loop.
//!
//! The callbacks never consume an event: every one of them ends by returning
//! what `CallNextHookEx` returns, and `scripts/guards/no-consuming-hook.sh`
//! rejects any other return in this file. Holding Ctrl+Win would open the
//! Start menu on release, so when a take starts with Win held, Tandem taps
//! the unassigned key 0xE8; the shell then treats Win as part of a chord.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::binding::Modifier;
use super::keymaps::vk_to_raw;
use super::{
    now_ms, Bindings, HotkeyBackend, HotkeyError, HotkeyStatus, KeyEvent, KeyState, Phase, RawKey,
    Which,
};

/// The unassigned virtual key tapped to keep the Start menu shut.
const MASK_VK: u16 = 0xE8;

struct Ctx {
    state: KeyState,
    tx: mpsc::Sender<KeyEvent>,
    esc: Arc<AtomicBool>,
    mask_win: bool,
}

fn ctx() -> &'static Mutex<Option<Ctx>> {
    static CTX: OnceLock<Mutex<Option<Ctx>>> = OnceLock::new();
    CTX.get_or_init(|| Mutex::new(None))
}

/// Records one transition. Never decides the hook's return value.
fn observe(key: RawKey, down: bool, injected: bool) {
    if injected {
        return;
    }
    let Ok(mut guard) = ctx().lock() else { return };
    let Some(c) = guard.as_mut() else { return };
    let esc = c.esc.load(Ordering::Acquire);
    for e in c.state.transition(key, down, esc, now_ms()) {
        if e.which == Which::Hold && e.phase == Phase::Down && c.mask_win {
            std::thread::spawn(tap_mask_key);
        }
        let _ = c.tx.send(e);
    }
}

fn tap_mask_key() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MASK_VK),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: a valid array of two keyboard INPUTs and its element size.
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if down || up {
            if let Some(key) = vk_to_raw(info.vkCode) {
                observe(key, down, info.flags.0 & LLKHF_INJECTED.0 != 0);
            }
        }
    }
    // SAFETY: passing the hook's own arguments on, as the hook chain requires.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let msg = wparam.0 as u32;
        if msg == WM_XBUTTONDOWN || msg == WM_XBUTTONUP {
            // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let button = (info.mouseData >> 16) & 0xffff;
            let key = if button == 1 {
                Some(RawKey::Mouse4)
            } else if button == 2 {
                Some(RawKey::Mouse5)
            } else {
                None
            };
            if let Some(key) = key {
                observe(key, msg == WM_XBUTTONDOWN, info.flags & 1 != 0);
            }
        }
    }
    // SAFETY: passing the hook's own arguments on, as the hook chain requires.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub struct WinLowLevelHook {
    thread: AtomicU32,
    esc: Arc<AtomicBool>,
    status: Mutex<HotkeyStatus>,
}

impl WinLowLevelHook {
    pub fn new() -> Self {
        Self {
            thread: AtomicU32::new(0),
            esc: Arc::new(AtomicBool::new(false)),
            status: Mutex::new(HotkeyStatus::NotRunning),
        }
    }

    fn set_status(&self, s: HotkeyStatus) {
        if let Ok(mut g) = self.status.lock() {
            *g = s;
        }
    }
}

impl Default for WinLowLevelHook {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyBackend for WinLowLevelHook {
    fn start(
        &self,
        bindings: &Bindings,
        tx: mpsc::Sender<KeyEvent>,
    ) -> Result<HotkeyStatus, HotkeyError> {
        self.stop();
        let mask_win = bindings
            .hold
            .as_ref()
            .is_some_and(|b| b.modifiers.contains(&Modifier::Meta));
        if let Ok(mut g) = ctx().lock() {
            *g = Some(Ctx {
                state: KeyState::new(bindings),
                tx,
                esc: self.esc.clone(),
                mask_win,
            });
        }
        let mouse = [&bindings.hold, &bindings.toggle].iter().any(|b| {
            b.as_ref().is_some_and(|b| {
                matches!(
                    b.trigger,
                    Some(super::binding::Trigger::Mouse4 | super::binding::Trigger::Mouse5)
                )
            })
        });
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dictate-hotkey-win".into())
            .spawn(move || {
                // SAFETY: plain Win32 calls on this thread; the hooks are
                // removed before the thread ends.
                unsafe {
                    let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
                    let ms = if mouse {
                        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0).ok()
                    } else {
                        None
                    };
                    let kb = match kb {
                        Ok(h) => h,
                        Err(e) => {
                            let _ = ready_tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(kb);
                    if let Some(ms) = ms {
                        let _ = UnhookWindowsHookEx(ms);
                    }
                }
            })
            .map_err(|e| HotkeyError::Failed(e.to_string()))?;
        match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(id)) => {
                self.thread.store(id, Ordering::Release);
                self.set_status(HotkeyStatus::Active);
                Ok(HotkeyStatus::Active)
            }
            Ok(Err(e)) => {
                self.set_status(HotkeyStatus::NotRunning);
                Err(HotkeyError::Failed(e))
            }
            Err(_) => Err(HotkeyError::Failed("the hook thread didn't start".into())),
        }
    }

    fn stop(&self) {
        let id = self.thread.swap(0, Ordering::AcqRel);
        if id != 0 {
            // SAFETY: posting WM_QUIT to our own hook thread ends its loop.
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Ok(mut g) = ctx().lock() {
            *g = None;
        }
        self.set_status(HotkeyStatus::NotRunning);
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
