//! IOKit/IOHIDManager standalone probe.
//!
//! Opens HID gamepad devices directly via IOHIDManager and measures the
//! delta between sequential value-change callbacks per HID element.
//!
//! Compare this to the SDL2 baseline measured via PADJUTSU_METRICS=1.
//! If IOKit min dt is ~4-8ms vs SDL2's 22ms, the IOKit path is worth the migration.
//!
//! Usage: `cargo run --example iokit_probe -p padjutsu-gamepad --release`
//! Move sticks for 30 seconds, then Ctrl-C.

#![cfg(target_os = "macos")]

#[link(name = "IOKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {}

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::time::{Duration, Instant};

// --- Core Foundation FFI ---

#[allow(non_camel_case_types)]
type CFTypeRef = *const c_void;
#[allow(non_camel_case_types)]
type CFAllocatorRef = *const c_void;
#[allow(non_camel_case_types)]
type CFStringRef = *const c_void;
#[allow(non_camel_case_types)]
type CFNumberRef = *const c_void;
#[allow(non_camel_case_types)]
type CFDictionaryRef = *const c_void;
#[allow(non_camel_case_types)]
type CFMutableDictionaryRef = *mut c_void;
#[allow(non_camel_case_types)]
type CFRunLoopRef = *const c_void;
#[allow(non_camel_case_types)]
type CFRunLoopMode = CFStringRef;
#[allow(non_camel_case_types)]
type CFIndex = isize;
#[allow(non_camel_case_types)]
type CFAbsoluteTime = f64;
#[allow(non_camel_case_types)]
type CFTimeInterval = f64;
#[allow(non_camel_case_types)]
type Boolean = u8;
#[allow(non_camel_case_types)]
type CFNumberType = c_int;

const K_CF_NUMBER_INT_TYPE: CFNumberType = 9;

extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFRunLoopDefaultMode: CFStringRef;

    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        cstr: *const u8,
        encoding: u32,
    ) -> CFStringRef;
    fn CFNumberCreate(
        alloc: CFAllocatorRef,
        number_type: CFNumberType,
        value_ptr: *const c_void,
    ) -> CFNumberRef;
    fn CFDictionaryCreateMutable(
        alloc: CFAllocatorRef,
        capacity: CFIndex,
        key_cb: *const c_void,
        val_cb: *const c_void,
    ) -> CFMutableDictionaryRef;
    fn CFDictionarySetValue(
        dict: CFMutableDictionaryRef,
        key: CFTypeRef,
        value: CFTypeRef,
    );
    fn CFRelease(cf: CFTypeRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRun();
    fn CFRunLoopRunInMode(
        mode: CFRunLoopMode,
        seconds: CFTimeInterval,
        return_after_source_handled: Boolean,
    ) -> i32;
}

// --- IOKit / IOHIDManager FFI ---

#[allow(non_camel_case_types)]
type IOHIDManagerRef = *const c_void;
#[allow(non_camel_case_types)]
type IOHIDDeviceRef = *const c_void;
#[allow(non_camel_case_types)]
type IOHIDElementRef = *const c_void;
#[allow(non_camel_case_types)]
type IOHIDValueRef = *const c_void;
#[allow(non_camel_case_types)]
type IOReturn = i32;
#[allow(non_camel_case_types)]
type IOOptionBits = u32;

const K_IOHID_OPTIONS_TYPE_NONE: IOOptionBits = 0;
const K_IORETURN_SUCCESS: IOReturn = 0;

// HID Usage Pages and Usages (Generic Desktop)
const K_HID_PAGE_GENERIC_DESKTOP: i32 = 0x01;
const K_HID_USAGE_GAMEPAD: i32 = 0x05;
const K_HID_USAGE_JOYSTICK: i32 = 0x04;
const K_HID_USAGE_MULTI_AXIS: i32 = 0x08;

extern "C" {
    fn IOHIDManagerCreate(alloc: CFAllocatorRef, options: IOOptionBits) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatchingMultiple(manager: IOHIDManagerRef, multiple: CFTypeRef);
    fn IOHIDManagerRegisterInputValueCallback(
        manager: IOHIDManagerRef,
        callback: extern "C" fn(*mut c_void, IOReturn, *mut c_void, IOHIDValueRef),
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        run_loop_mode: CFRunLoopMode,
    );
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDManagerClose(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;

    fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> CFIndex;
    fn IOHIDValueGetTimeStamp(value: IOHIDValueRef) -> u64;

    fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetDevice(element: IOHIDElementRef) -> IOHIDDeviceRef;
}

unsafe fn cf_str(s: &str) -> CFStringRef {
    let c = std::ffi::CString::new(s).unwrap();
    CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr() as *const u8, 0x08000100)
}

unsafe fn cf_number_int(v: i32) -> CFNumberRef {
    CFNumberCreate(
        kCFAllocatorDefault,
        K_CF_NUMBER_INT_TYPE,
        &v as *const i32 as *const c_void,
    )
}

unsafe fn matching_dict(usage_page: i32, usage: i32) -> CFDictionaryRef {
    let dict = CFDictionaryCreateMutable(
        kCFAllocatorDefault,
        2,
        &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
        &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
    );
    let key_page = cf_str("DeviceUsagePage");
    let key_usage = cf_str("DeviceUsage");
    let v_page = cf_number_int(usage_page);
    let v_usage = cf_number_int(usage);
    CFDictionarySetValue(dict, key_page, v_page);
    CFDictionarySetValue(dict, key_usage, v_usage);
    CFRelease(key_page);
    CFRelease(key_usage);
    CFRelease(v_page);
    CFRelease(v_usage);
    dict as CFDictionaryRef
}

// --- Per-element histogram ---

#[derive(Default)]
struct ElemStats {
    last_t: Option<Instant>,
    n: u64,
    pauses: u64,
    sum_us: u128,
    min_us: u64,
    max_us: u64,
    bucket_under_2ms: u64,
    bucket_under_4ms: u64,
    bucket_under_6ms: u64,
    bucket_under_8ms: u64,
    bucket_under_12ms: u64,
    bucket_under_16ms: u64,
    bucket_under_22ms: u64,
    bucket_under_30ms: u64,
}

const POLL_CUTOFF_US: u64 = 30_000;

impl ElemStats {
    fn observe(&mut self, t: Instant) {
        if let Some(prev) = self.last_t {
            let dt = t.saturating_duration_since(prev);
            let us = dt.as_micros() as u64;
            if us > POLL_CUTOFF_US {
                self.pauses += 1;
            } else {
                self.n += 1;
                self.sum_us += us as u128;
                if self.min_us == 0 || us < self.min_us {
                    self.min_us = us;
                }
                if us > self.max_us {
                    self.max_us = us;
                }
                if us < 2_000 {
                    self.bucket_under_2ms += 1;
                } else if us < 4_000 {
                    self.bucket_under_4ms += 1;
                } else if us < 6_000 {
                    self.bucket_under_6ms += 1;
                } else if us < 8_000 {
                    self.bucket_under_8ms += 1;
                } else if us < 12_000 {
                    self.bucket_under_12ms += 1;
                } else if us < 16_000 {
                    self.bucket_under_16ms += 1;
                } else if us < 22_000 {
                    self.bucket_under_22ms += 1;
                } else {
                    self.bucket_under_30ms += 1;
                }
            }
        }
        self.last_t = Some(t);
    }
}

thread_local! {
    static STATS: RefCell<HashMap<(u32, u32), ElemStats>> =
        RefCell::new(HashMap::new());
    static REPORT_AT: RefCell<Instant> = RefCell::new(Instant::now());
}

extern "C" fn input_value_callback(
    _ctx: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    value: IOHIDValueRef,
) {
    let now = Instant::now();
    unsafe {
        let element = IOHIDValueGetElement(value);
        if element.is_null() {
            return;
        }
        let usage_page = IOHIDElementGetUsagePage(element);
        let usage = IOHIDElementGetUsage(element);
        STATS.with(|s| {
            let mut map = s.borrow_mut();
            let entry = map.entry((usage_page, usage)).or_default();
            entry.observe(now);
        });
    }
}

fn usage_label(page: u32, usage: u32) -> String {
    // Generic Desktop usages relevant for gamepads
    match (page, usage) {
        (0x01, 0x30) => "X (LStick X)".into(),
        (0x01, 0x31) => "Y (LStick Y)".into(),
        (0x01, 0x32) => "Z (RStick X / LT)".into(),
        (0x01, 0x33) => "Rx (RStick Y? / LT?)".into(),
        (0x01, 0x34) => "Ry".into(),
        (0x01, 0x35) => "Rz".into(),
        (0x01, 0x39) => "Hat (D-Pad)".into(),
        (0x09, u) => format!("Button {}", u),
        (p, u) => format!("page=0x{:02X} usage=0x{:02X}", p, u),
    }
}

fn report() {
    println!("\n=========== IOKit probe report ===========");
    STATS.with(|s| {
        let map = s.borrow();
        let mut items: Vec<_> = map.iter().collect();
        items.sort_by_key(|((p, u), _)| (*p, *u));
        for ((page, usage), stats) in items {
            if stats.n == 0 && stats.pauses == 0 {
                continue;
            }
            let avg = if stats.n > 0 {
                (stats.sum_us / stats.n as u128) as u64
            } else {
                0
            };
            println!(
                "{:30}  active_n={:5} pauses={:4} min={:>5}us avg={:>5}us max={:>5}us | <2={} <4={} <6={} <8={} <12={} <16={} <22={} <30={}",
                usage_label(*page, *usage),
                stats.n,
                stats.pauses,
                stats.min_us,
                avg,
                stats.max_us,
                stats.bucket_under_2ms,
                stats.bucket_under_4ms,
                stats.bucket_under_6ms,
                stats.bucket_under_8ms,
                stats.bucket_under_12ms,
                stats.bucket_under_16ms,
                stats.bucket_under_22ms,
                stats.bucket_under_30ms,
            );
        }
    });
    println!("==========================================\n");
}

fn main() {
    println!("[iokit-probe] starting. Move sticks for ~30s, then Ctrl-C.");

    unsafe {
        let manager = IOHIDManagerCreate(kCFAllocatorDefault, K_IOHID_OPTIONS_TYPE_NONE);
        if manager.is_null() {
            eprintln!("IOHIDManagerCreate failed");
            return;
        }

        // Match gamepad/joystick/multi-axis devices
        let m1 = matching_dict(K_HID_PAGE_GENERIC_DESKTOP, K_HID_USAGE_GAMEPAD);
        let m2 = matching_dict(K_HID_PAGE_GENERIC_DESKTOP, K_HID_USAGE_JOYSTICK);
        let m3 = matching_dict(K_HID_PAGE_GENERIC_DESKTOP, K_HID_USAGE_MULTI_AXIS);

        // Pack all three into a CFArray. Easier path: just match gamepad first.
        // Build CFArray manually via Core Foundation:
        extern "C" {
            fn CFArrayCreate(
                alloc: CFAllocatorRef,
                values: *const CFTypeRef,
                count: CFIndex,
                callbacks: *const c_void,
            ) -> CFTypeRef;
            static kCFTypeArrayCallBacks: c_void;
        }
        let arr_items: [CFTypeRef; 3] = [m1 as _, m2 as _, m3 as _];
        let arr = CFArrayCreate(
            kCFAllocatorDefault,
            arr_items.as_ptr(),
            3,
            &kCFTypeArrayCallBacks as *const _ as *const c_void,
        );
        IOHIDManagerSetDeviceMatchingMultiple(manager, arr);
        CFRelease(arr);
        CFRelease(m1 as _);
        CFRelease(m2 as _);
        CFRelease(m3 as _);

        IOHIDManagerRegisterInputValueCallback(
            manager,
            input_value_callback,
            std::ptr::null_mut(),
        );

        IOHIDManagerScheduleWithRunLoop(
            manager,
            CFRunLoopGetCurrent(),
            kCFRunLoopDefaultMode,
        );

        let res = IOHIDManagerOpen(manager, K_IOHID_OPTIONS_TYPE_NONE);
        if res != K_IORETURN_SUCCESS {
            eprintln!("IOHIDManagerOpen failed: {res:#x}");
            return;
        }

        // Set Ctrl-C
        let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, std::sync::atomic::Ordering::SeqCst);
        }).expect("ctrlc handler");

        let started = Instant::now();
        let mut last_report = Instant::now();
        while running.load(std::sync::atomic::Ordering::SeqCst) {
            // Run the CFRunLoop in 100ms slices, then check for shutdown / report.
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0);
            if last_report.elapsed() >= Duration::from_secs(5) {
                report();
                last_report = Instant::now();
            }
            // Hard cap 60s
            if started.elapsed() >= Duration::from_secs(60) {
                println!("[iokit-probe] 60s elapsed, stopping.");
                break;
            }
        }

        report();
        IOHIDManagerClose(manager, K_IOHID_OPTIONS_TYPE_NONE);
        CFRelease(manager as _);
    }
}
