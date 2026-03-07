use std::sync::Arc;

use gamacros_control::KeyCombo;
use gamacros_gamepad::ControllerId;
use gamacros_workspace::{Macros, MouseButton, MouseClickType, RawModifierKey};

#[derive(Debug, Clone)]
pub enum Effect {
    KeyPress(KeyCombo),
    KeyRelease(KeyCombo),
    KeyTap(KeyCombo),
    Macros(Arc<Macros>),
    Shell(String),
    MouseClick {
        button: MouseButton,
        click_type: MouseClickType,
    },
    MousePress {
        button: MouseButton,
    },
    MouseRelease {
        button: MouseButton,
    },
    MouseMove {
        dx: i32,
        dy: i32,
    },
    Scroll {
        h: f64,
        v: f64,
    },
    Rumble {
        id: ControllerId,
        ms: u32,
    },
    RawModifierPress(RawModifierKey),
    RawModifierRelease(RawModifierKey),
}
