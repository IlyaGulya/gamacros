use std::time::Instant;

use crate::app::Effect;
use crate::domain::stick_state::StickState;
use crate::domain::{ControllerRuntimeState, RuntimeMode};

pub enum ModeTransition {
    Set(RuntimeMode),
}

pub enum ShellTransition {
    Set(Option<Box<str>>),
}

pub struct ControllerTransition {
    pub id: padjutsu_gamepad::ControllerId,
    pub next_state: Option<ControllerRuntimeState>,
}

pub struct StickTransition {
    pub controller_id: padjutsu_gamepad::ControllerId,
    pub previous: Option<StickState>,
    pub next: StickState,
}

pub enum WakeTransition {
    Reschedule,
    EnableFastModeUntil(Instant),
    DisableFastMode,
}

pub struct Transition {
    pub effects: Vec<Effect>,
    pub shell: Option<ShellTransition>,
    pub wake: Vec<WakeTransition>,
    pub controller_updates: Vec<ControllerTransition>,
    pub stick_updates: Vec<StickTransition>,
    pub mode: Option<ModeTransition>,
}

impl Transition {
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
            shell: None,
            wake: Vec::new(),
            controller_updates: Vec::new(),
            stick_updates: Vec::new(),
            mode: None,
        }
    }
}
