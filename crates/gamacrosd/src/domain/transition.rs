use crate::app::Effect;
use crate::domain::{ControllerRuntimeState, RuntimeMode, WakeIntent};

pub enum ModeTransition {
    Set(RuntimeMode),
}

pub enum ShellTransition {
    Set(Option<Box<str>>),
}

pub struct ControllerTransition {
    pub id: gamacros_gamepad::ControllerId,
    pub next_state: Option<ControllerRuntimeState>,
}

pub struct Transition {
    pub effects: Vec<Effect>,
    pub shell: Option<ShellTransition>,
    pub wake_intents: Vec<WakeIntent>,
    pub controller_updates: Vec<ControllerTransition>,
    pub mode: Option<ModeTransition>,
}

impl Transition {
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
            shell: None,
            wake_intents: Vec::new(),
            controller_updates: Vec::new(),
            mode: None,
        }
    }
}
