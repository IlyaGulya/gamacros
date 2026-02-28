use enigo::{Axis, Button, Coordinate, Direction, Enigo, InputResult, Mouse, NewConError, Settings};

use crate::KeyCombo;

#[cfg(target_os = "macos")]
mod raw_modifier {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    /// Device-specific flag bits (from IOKit NX headers).
    const NX_DEVICELCTLKEYMASK: u64 = 0x0000_0001;
    const NX_DEVICERCTLKEYMASK: u64 = 0x0000_2000;
    const NX_DEVICELSHIFTKEYMASK: u64 = 0x0000_0002;
    const NX_DEVICERSHIFTKEYMASK: u64 = 0x0000_0004;
    const NX_DEVICELCMDKEYMASK: u64 = 0x0000_0008;
    const NX_DEVICERCMDKEYMASK: u64 = 0x0000_0010;
    const NX_DEVICELALTKEYMASK: u64 = 0x0000_0020;
    const NX_DEVICERALTKEYMASK: u64 = 0x0000_0040;

    /// macOS virtual keycodes for modifier keys.
    pub const KC_CONTROL: u16 = 0x3B;
    pub const KC_RIGHT_CONTROL: u16 = 0x3E;
    pub const KC_SHIFT: u16 = 0x38;
    pub const KC_RIGHT_SHIFT: u16 = 0x3C;
    pub const KC_COMMAND: u16 = 0x37;
    pub const KC_RIGHT_COMMAND: u16 = 0x36;
    pub const KC_OPTION: u16 = 0x3A;
    pub const KC_RIGHT_OPTION: u16 = 0x3D;

    /// Returns (high-level CGEventFlags mask, device-specific mask) for a modifier keycode.
    fn modifier_flags(keycode: u16) -> Option<(CGEventFlags, u64)> {
        match keycode {
            KC_CONTROL => Some((CGEventFlags::CGEventFlagControl, NX_DEVICELCTLKEYMASK)),
            KC_RIGHT_CONTROL => Some((CGEventFlags::CGEventFlagControl, NX_DEVICERCTLKEYMASK)),
            KC_SHIFT => Some((CGEventFlags::CGEventFlagShift, NX_DEVICELSHIFTKEYMASK)),
            KC_RIGHT_SHIFT => Some((CGEventFlags::CGEventFlagShift, NX_DEVICERSHIFTKEYMASK)),
            KC_COMMAND => Some((CGEventFlags::CGEventFlagCommand, NX_DEVICELCMDKEYMASK)),
            KC_RIGHT_COMMAND => Some((CGEventFlags::CGEventFlagCommand, NX_DEVICERCMDKEYMASK)),
            KC_OPTION => Some((CGEventFlags::CGEventFlagAlternate, NX_DEVICELALTKEYMASK)),
            KC_RIGHT_OPTION => Some((CGEventFlags::CGEventFlagAlternate, NX_DEVICERALTKEYMASK)),
            _ => None,
        }
    }

    /// Post a FlagsChanged CGEvent, which is what macOS generates for real modifier keypresses.
    pub fn post_flags_changed(keycode: u16, pressed: bool) -> Result<(), String> {
        let (high_flag, dev_flag) = modifier_flags(keycode)
            .ok_or_else(|| format!("keycode 0x{keycode:02x} is not a modifier"))?;

        // Use CombinedSessionState so the event updates the global modifier state.
        // Apps like Freeflow may poll CGEventSourceFlagsState(CombinedSessionState)
        // rather than monitoring individual events.
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| "failed to create CGEventSource")?;

        // Create a FlagsChanged event.  The core_graphics crate exposes
        // `CGEvent::new_keyboard_event` which creates KeyDown/KeyUp.
        // For FlagsChanged we create a keyboard event and then override its type.
        let event = CGEvent::new_keyboard_event(source, keycode, pressed)
            .map_err(|_| "failed to create CGEvent")?;

        // Override event type to FlagsChanged (type 12).
        event.set_type(CGEventType::FlagsChanged);

        // Build the flags bitfield.
        let mut flags = CGEventFlags::CGEventFlagNonCoalesced;
        if pressed {
            flags.insert(high_flag);
            flags.insert(CGEventFlags::from_bits_retain(dev_flag));
        }
        // When releasing, flags should be empty (no modifier held).
        event.set_flags(flags);

        log::info!(
            "[raw_modifier] posting FlagsChanged keycode=0x{keycode:02x} pressed={pressed} flags=0x{:016x}",
            flags.bits()
        );

        // Post at HID level so the event goes through the full macOS input pipeline.
        // Using CombinedSessionState source ensures the global modifier state is updated.
        event.post(CGEventTapLocation::HID);
        Ok(())
    }
}

pub struct Performer {
    enigo: Enigo,
}

// SAFETY: This is safe because we're only accessing Enigo through a Mutex,
// which provides the necessary synchronization. The internal CGEventSource
// is only used on the thread that actually performs the key presses.
unsafe impl Send for Performer {}
unsafe impl Sync for Performer {}

impl Performer {
    /// Create a new performer.
    pub fn new() -> Result<Self, NewConError> {
        let settings = Settings::default();
        let enigo = Enigo::new(&settings)?;
        Ok(Self { enigo })
    }

    /// Perform key combo.
    /// This will press and release the keys in the key combo.
    pub fn perform(&mut self, key_combo: &KeyCombo) -> InputResult<()> {
        key_combo.perform(&mut self.enigo)
    }

    /// Press keys.
    pub fn press(&mut self, key_combo: &KeyCombo) -> InputResult<()> {
        key_combo.press(&mut self.enigo)
    }

    /// Release keys.
    pub fn release(&mut self, key_combo: &KeyCombo) -> InputResult<()> {
        key_combo.release(&mut self.enigo)
    }

    /// Move mouse.
    pub fn mouse_move(&mut self, x: i32, y: i32) -> InputResult<()> {
        self.enigo.move_mouse(x, y, Coordinate::Rel)
    }

    /// Scroll horizontally.
    /// Uses macOS specific smooth scrolling.
    #[cfg(target_os = "macos")]
    pub fn scroll_x(&mut self, value: i32) -> InputResult<()> {
        self.enigo.smooth_scroll(value, Axis::Horizontal)
    }

    /// Scroll vertically.
    /// Uses macOS specific smooth scrolling.
    #[cfg(target_os = "macos")]
    pub fn scroll_y(&mut self, value: i32) -> InputResult<()> {
        self.enigo.smooth_scroll(value, Axis::Vertical)
    }

    /// Fallback for non-macOS systems
    #[cfg(not(target_os = "macos"))]
    pub fn scroll_x(&mut self, value: i32) -> InputResult<()> {
        self.enigo.scroll(value, Axis::Horizontal)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn scroll_y(&mut self, value: i32) -> InputResult<()> {
        self.enigo.scroll(value, Axis::Vertical)
    }

    /// Click a mouse button.
    pub fn mouse_click(&mut self, button: Button) -> InputResult<()> {
        self.enigo.button(button, Direction::Click)
    }

    /// Double-click a mouse button.
    pub fn mouse_double_click(&mut self, button: Button) -> InputResult<()> {
        self.enigo.button(button, Direction::Click)?;
        self.enigo.button(button, Direction::Click)
    }

    /// Send a raw modifier key press via FlagsChanged CGEvent (macOS only).
    /// This is the correct event type for modifier keys — apps like Freeflow
    /// and SuperWhisper that listen for modifier-only keypresses need this.
    #[cfg(target_os = "macos")]
    pub fn raw_modifier_press(&mut self, keycode: u16) -> Result<(), String> {
        raw_modifier::post_flags_changed(keycode, true)
    }

    /// Send a raw modifier key release via FlagsChanged CGEvent (macOS only).
    #[cfg(target_os = "macos")]
    pub fn raw_modifier_release(&mut self, keycode: u16) -> Result<(), String> {
        raw_modifier::post_flags_changed(keycode, false)
    }
}
