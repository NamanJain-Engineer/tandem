//! macOS insertion (F-DI-06): the frontmost app (NSWorkspace) and its
//! Accessibility-focused element are captured; restoring raises the app
//! first, then focuses the element, and reads the frontmost app back; ⌘V
//! pastes. Needs Accessibility ("types the words in").

use std::ffi::{c_char, c_void, CString};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::{is_terminal_app, Board, Clip, InsertError, Platform, Target};

type Id = *mut c_void;
type Sel = *mut c_void;
type CFTypeRef = *const c_void;
type AXUIElementRef = *mut c_void;

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFTypeRef,
        value: CFTypeRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
}

const UTF8: u32 = 0x0800_0100;
const ACTIVATE_IGNORING_OTHER_APPS: usize = 1 << 1;

fn sel(name: &str) -> Sel {
    let c = CString::new(name).unwrap_or_default();
    // SAFETY: registering a selector name is always safe.
    unsafe { sel_registerName(c.as_ptr()) }
}

fn class(name: &str) -> Id {
    let c = CString::new(name).unwrap_or_default();
    // SAFETY: looks a class up by name; null when absent.
    unsafe { objc_getClass(c.as_ptr()) }
}

/// `[obj sel]` returning an object.
unsafe fn send_id(obj: Id, s: Sel) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s) }
}

/// `[obj sel]` returning an int (`pid_t`).
unsafe fn send_i32(obj: Id, s: Sel) -> i32 {
    let f: unsafe extern "C" fn(Id, Sel) -> i32 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s) }
}

/// `[cls sel:pid]` returning an object.
unsafe fn send_id_i32(obj: Id, s: Sel, arg: i32) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, i32) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, arg) }
}

/// `[obj sel:options]` returning BOOL.
unsafe fn send_bool_usize(obj: Id, s: Sel, arg: usize) -> bool {
    let f: unsafe extern "C" fn(Id, Sel, usize) -> i8 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, arg) != 0 }
}

unsafe fn nsstring(s: Id) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut buf = vec![0 as c_char; 512];
    // SAFETY: NSString is toll-free bridged to CFString.
    let ok = unsafe { CFStringGetCString(s as CFTypeRef, buf.as_mut_ptr(), 512, UTF8) };
    if ok == 0 {
        return None;
    }
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    Some(String::from_utf8_lossy(&bytes).to_string())
}

fn cfstr(s: &str) -> CFTypeRef {
    let c = CString::new(s).unwrap_or_default();
    // SAFETY: creates a CFString the caller releases.
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
}

/// The frontmost app: (pid, bundle id, name).
fn frontmost() -> Option<(i32, Option<String>, Option<String>)> {
    // SAFETY: AppKit getters on shared objects; nothing is retained.
    unsafe {
        let ws = send_id(class("NSWorkspace"), sel("sharedWorkspace"));
        if ws.is_null() {
            return None;
        }
        let app = send_id(ws, sel("frontmostApplication"));
        if app.is_null() {
            return None;
        }
        let pid = send_i32(app, sel("processIdentifier"));
        let bundle = nsstring(send_id(app, sel("bundleIdentifier")));
        let name = nsstring(send_id(app, sel("localizedName")));
        Some((pid, bundle, name))
    }
}

fn activate(pid: i32) -> bool {
    // SAFETY: AppKit calls on an autoreleased NSRunningApplication.
    unsafe {
        let app = send_id_i32(
            class("NSRunningApplication"),
            sel("runningApplicationWithProcessIdentifier:"),
            pid,
        );
        !app.is_null()
            && send_bool_usize(
                app,
                sel("activateWithOptions:"),
                ACTIVATE_IGNORING_OTHER_APPS,
            )
    }
}

pub fn accessibility_granted() -> bool {
    // SAFETY: a plain query.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// A retained AX element owned by the platform.
struct Element(AXUIElementRef);
// SAFETY: AX elements are CF objects, usable from any thread.
unsafe impl Send for Element {}

impl Drop for Element {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: we hold one retain on it.
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }
}

fn focused_element(pid: i32) -> Option<Element> {
    // SAFETY: AX copies return retained objects, released here or by Element.
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let attr = cfstr("AXFocusedUIElement");
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(app, attr, &mut value);
        CFRelease(attr);
        CFRelease(app as CFTypeRef);
        (err == 0 && !value.is_null()).then(|| Element(value as AXUIElementRef))
    }
}

fn system_focused_pid() -> Option<i32> {
    frontmost().map(|(pid, ..)| pid)
}

pub struct MacPlatform {
    board: Board,
    element: Mutex<Option<(u64, Element)>>,
    next: Mutex<u64>,
}

impl MacPlatform {
    pub fn new() -> Self {
        // Touch the system-wide element once so AX is initialised.
        // SAFETY: created and released immediately.
        unsafe {
            let sys = AXUIElementCreateSystemWide();
            if !sys.is_null() {
                CFRelease(CFRetain(sys as CFTypeRef));
                CFRelease(sys as CFTypeRef);
            }
        }
        Self {
            board: Board::new(),
            element: Mutex::new(None),
            next: Mutex::new(1),
        }
    }
}

impl Default for MacPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for MacPlatform {
    fn capture_target(&self) -> Result<Option<Target>, InsertError> {
        let Some((pid, bundle, name)) = frontmost() else {
            return Ok(None);
        };
        let element_id = if accessibility_granted() {
            focused_element(pid).map(|el| {
                let mut n = self.next.lock();
                *n += 1;
                let id = *n;
                *self.element.lock() = Some((id, el));
                id
            })
        } else {
            None
        };
        Ok(Some(Target {
            terminal: bundle.as_deref().is_some_and(is_terminal_app),
            app_id: bundle,
            app_name: name,
            window: pid as u64,
            element: element_id.unwrap_or(0),
            elevated: false,
            own: pid as u32 == std::process::id(),
        }))
    }

    fn restore_focus(&self, target: &Target) -> Result<(), InsertError> {
        let pid = target.window as i32;
        if system_focused_pid() != Some(pid) && !activate(pid) {
            return Err(InsertError::TargetGone);
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut front = false;
        while Instant::now() < deadline {
            if system_focused_pid() == Some(pid) {
                front = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !front {
            return Err(InsertError::Focus);
        }
        if let Some((id, el)) = &*self.element.lock() {
            if *id == target.element {
                let attr = cfstr("AXFocused");
                // SAFETY: setting a boolean attribute on a retained element.
                unsafe {
                    AXUIElementSetAttributeValue(el.0, attr, kCFBooleanTrue);
                    CFRelease(attr);
                }
            }
        }
        Ok(())
    }

    fn paste(&self, _terminal: bool) -> Result<(), InsertError> {
        super::chord(&[enigo::Key::Meta], enigo::Key::Unicode('v'))
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
        true
    }
}
