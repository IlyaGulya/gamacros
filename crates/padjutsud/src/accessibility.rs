use std::ffi::c_void;
use std::ptr;

use crate::print_info;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);

    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFBooleanTrue: *const c_void;

    // "kAXTrustedCheckOptionPrompt" lives in ApplicationServices
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

fn accessibility_options(prompt: bool) -> *const c_void {
    if !prompt {
        return ptr::null();
    }

    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _,
            &kCFTypeDictionaryValueCallBacks as *const _,
        )
    }
}

fn check_accessibility(prompt: bool) -> bool {
    unsafe {
        let options = accessibility_options(prompt);
        let trusted = AXIsProcessTrustedWithOptions(options);
        if !options.is_null() {
            CFRelease(options);
        }
        trusted
    }
}

pub fn is_trusted() -> bool {
    check_accessibility(false)
}

pub fn request_if_needed() -> bool {
    let trusted = check_accessibility(true);
    if !trusted {
        print_info!(
            "Accessibility permission required. \
             Please enable padjutsud in System Settings -> Privacy & Security -> Accessibility."
        );
    }
    trusted
}

pub fn log_missing_permission() {
    print_info!(
        "Accessibility permission required. \
         Waiting for padjutsud to be allowed in System Settings -> Privacy & Security -> Accessibility."
    );
}
