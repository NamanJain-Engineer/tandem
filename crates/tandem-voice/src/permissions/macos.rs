//! The macOS side of [`super::PrivacyProbe`]: AVFoundation for the
//! microphone, IOKit for Input Monitoring, the Accessibility API for
//! Accessibility. Type-checked from Linux (`scripts/macos-check.sh`); run on
//! a Mac (HUMAN_TODO "macOS first run").

use std::ffi::{c_char, c_ulong, c_void, CString};
use std::sync::OnceLock;

use super::{from_av_status, from_ax_trusted, from_iohid, Access, Privacy, PrivacyProbe};

type Id = *mut c_void;
type Sel = *mut c_void;
type CFTypeRef = *const c_void;

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: Id;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

#[link(name = "System", kind = "dylib")]
extern "C" {
    static _NSConcreteGlobalBlock: c_void;
}

/// `kIOHIDRequestTypeListenEvent`: Input Monitoring.
const LISTEN_EVENT: u32 = 1;

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

pub fn input_monitoring() -> Access {
    // SAFETY: a plain IOKit query.
    from_iohid(unsafe { IOHIDCheckAccess(LISTEN_EVENT) })
}

pub fn microphone() -> Access {
    let cls = class("AVCaptureDevice");
    if cls.is_null() {
        return Access::NotAsked;
    }
    // SAFETY: `+[AVCaptureDevice authorizationStatusForMediaType:]` takes an
    // NSString constant and returns an NSInteger.
    let status = unsafe {
        let f: unsafe extern "C" fn(Id, Sel, Id) -> isize =
            std::mem::transmute(objc_msgSend as *const ());
        f(
            cls,
            sel("authorizationStatusForMediaType:"),
            AVMediaTypeAudio,
        )
    };
    from_av_status(status)
}

pub fn accessibility() -> Access {
    from_ax_trusted(crate::insert::macos::accessibility_granted())
}

/// A global block (no captures) for `completionHandler:`. The answer is
/// read back by polling `microphone()`, so it does nothing.
#[repr(C)]
struct Block {
    isa: *const c_void,
    flags: i32,
    reserved: i32,
    invoke: unsafe extern "C" fn(*const Block, i8),
    descriptor: *const BlockDescriptor,
}

#[repr(C)]
struct BlockDescriptor {
    reserved: c_ulong,
    size: c_ulong,
}

const BLOCK_IS_GLOBAL: i32 = 1 << 28;

unsafe extern "C" fn ignore_answer(_block: *const Block, _granted: i8) {}

/// The block's address, made once and never freed (a global block is never
/// copied or released by the runtime).
fn completion_block() -> *const Block {
    static BLOCK: OnceLock<usize> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        let descriptor = Box::leak(Box::new(BlockDescriptor {
            reserved: 0,
            size: std::mem::size_of::<Block>() as c_ulong,
        }));
        let block = Box::leak(Box::new(Block {
            // Only the symbol's address is taken.
            isa: std::ptr::addr_of!(_NSConcreteGlobalBlock),
            flags: BLOCK_IS_GLOBAL,
            reserved: 0,
            invoke: ignore_answer,
            descriptor,
        }));
        block as *const Block as usize
    }) as *const Block
}

fn request_microphone() {
    let cls = class("AVCaptureDevice");
    if cls.is_null() {
        return;
    }
    // SAFETY: `+[AVCaptureDevice requestAccessForMediaType:completionHandler:]`
    // with an NSString constant and a global block that outlives the call.
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel, Id, *const Block) =
            std::mem::transmute(objc_msgSend as *const ());
        f(
            cls,
            sel("requestAccessForMediaType:completionHandler:"),
            AVMediaTypeAudio,
            completion_block(),
        );
    }
}

fn request_accessibility() {
    // SAFETY: a one-entry CFDictionary {kAXTrustedCheckOptionPrompt: true},
    // released after the call.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks),
            std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks),
        );
        if options.is_null() {
            return;
        }
        AXIsProcessTrustedWithOptions(options);
        CFRelease(options);
    }
}

pub struct MacPrivacy;

impl PrivacyProbe for MacPrivacy {
    fn access(&self, what: Privacy) -> Access {
        match what {
            Privacy::Microphone => microphone(),
            Privacy::InputMonitoring => input_monitoring(),
            Privacy::Accessibility => accessibility(),
        }
    }

    fn request(&self, what: Privacy) {
        match what {
            Privacy::Microphone => request_microphone(),
            // SAFETY: shows the system prompt; returns at once.
            Privacy::InputMonitoring => {
                unsafe { IOHIDRequestAccess(LISTEN_EVENT) };
            }
            Privacy::Accessibility => request_accessibility(),
        }
    }
}
