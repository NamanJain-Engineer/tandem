//! Windows insertion (F-DI-06): the foreground window and its focused child
//! are captured; `SetForegroundWindow` gives focus back (a benign Alt tap
//! lifts the foreground lock when it refuses), confirmed by reading the
//! foreground back; Ctrl+V pastes. The clipboard isn't restored, so a
//! clipboard-history app can't capture the text twice. An elevated target
//! can't receive our input (UIPI), so it gets the text on the clipboard and
//! a warning that names it.

use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
    GUITHREADINFO,
};

use super::{is_terminal_app, Board, Clip, InsertError, Platform, Target, SYNTHETIC_MARK};

pub struct WinPlatform {
    board: Board,
}

impl WinPlatform {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
        }
    }
}

impl Default for WinPlatform {
    fn default() -> Self {
        Self::new()
    }
}

fn elevated(process: HANDLE) -> Option<bool> {
    // SAFETY: token handle opened and closed here; the out-struct is sized.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut info = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut info as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok.then_some(info.TokenIsElevated != 0)
    }
}

/// The process's executable name, and whether it runs elevated while we don't.
fn process_info(pid: u32) -> (Option<String>, bool) {
    // SAFETY: the process handle is opened and closed here.
    unsafe {
        let ours = elevated(GetCurrentProcess()).unwrap_or(false);
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            // Denied: almost always an elevated process seen from a normal one.
            return (None, !ours);
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let name = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()
        .map(|_| String::from_utf16_lossy(&buf[..len as usize]))
        .and_then(|path| path.rsplit('\\').next().map(|s| s.to_lowercase()));
        let theirs = elevated(handle).unwrap_or(false);
        let _ = CloseHandle(handle);
        (name, theirs && !ours)
    }
}

fn alt_tap() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MENU,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTHETIC_MARK as usize,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: a valid array of keyboard INPUTs and its element size.
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

impl Platform for WinPlatform {
    fn capture_target(&self) -> Result<Option<Target>, InsertError> {
        // SAFETY: read-only window queries.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return Ok(None);
            }
            let mut pid = 0u32;
            let thread = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut gui = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            let focus = if GetGUIThreadInfo(thread, &mut gui).is_ok() {
                gui.hwndFocus.0 as u64
            } else {
                0
            };
            let (exe, is_elevated) = process_info(pid);
            let own = pid == std::process::id();
            Ok(Some(Target {
                terminal: exe.as_deref().is_some_and(is_terminal_app),
                app_name: exe
                    .as_deref()
                    .map(|e| e.trim_end_matches(".exe").to_string()),
                app_id: exe,
                window: hwnd.0 as u64,
                element: focus,
                elevated: is_elevated,
                own,
            }))
        }
    }

    fn restore_focus(&self, target: &Target) -> Result<(), InsertError> {
        let hwnd = HWND(target.window as *mut core::ffi::c_void);
        // SAFETY: window calls on a handle we only compare and activate.
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return Err(InsertError::TargetGone);
            }
            if GetForegroundWindow() == hwnd {
                return Ok(());
            }
            if !SetForegroundWindow(hwnd).as_bool() {
                // The foreground lock: an Alt tap counts as user input.
                alt_tap();
                let _ = SetForegroundWindow(hwnd);
            }
            let deadline = Instant::now() + Duration::from_millis(400);
            while Instant::now() < deadline {
                if GetForegroundWindow() == hwnd {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Err(InsertError::Focus)
    }

    fn paste(&self, _terminal: bool) -> Result<(), InsertError> {
        super::chord(&[enigo::Key::Control], enigo::Key::Unicode('v'))
    }

    fn read_clipboard(&self) -> Result<Clip, InsertError> {
        self.board.read()
    }

    fn write_clipboard(&self, text: &str) -> Result<(), InsertError> {
        self.board.write(text)
    }

    fn restore_clipboard(&self, clip: &Clip) -> Result<(), InsertError> {
        self.board.restore(clip)
    }

    fn restores_clipboard(&self) -> bool {
        false
    }
}
