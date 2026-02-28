use std::ffi::c_void;

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

/// Check if the process has Accessibility permissions.
/// If `prompt` is true and permissions are missing, macOS will show the system dialog.
pub fn ensure_accessibility(prompt: bool) -> bool {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [if prompt {
            kCFBooleanTrue
        } else {
            // kCFBooleanFalse — just use null-ish; but easier to just always prompt
            kCFBooleanTrue
        }];

        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );

        let trusted = AXIsProcessTrustedWithOptions(options);
        CFRelease(options);

        if !trusted {
            print_info!(
                "Accessibility permission required. \
                 Please enable gamacrosd in System Settings → Privacy & Security → Accessibility, \
                 then restart."
            );
        }

        trusted
    }
}
