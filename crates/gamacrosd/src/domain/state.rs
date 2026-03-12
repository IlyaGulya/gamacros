use ahash::AHashMap;
use gamacros_gamepad::ControllerId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Booting,
    AwaitingProfile,
    Active,
    ShuttingDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerMode {
    ConnectedIdle,
    ButtonsActive,
    AxisActive,
    MixedInput,
}

pub struct RuntimeState {
    mode: RuntimeMode,
    controllers: AHashMap<ControllerId, ControllerMode>,
}

impl RuntimeState {
    pub fn new(mode: RuntimeMode) -> Self {
        Self {
            mode,
            controllers: AHashMap::new(),
        }
    }

    pub fn mode(&self) -> RuntimeMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: RuntimeMode) {
        self.mode = mode;
    }

    pub fn allows_input_actions(&self) -> bool {
        matches!(self.mode, RuntimeMode::Active)
    }

    pub fn handles_timer_wake(&self) -> bool {
        matches!(self.mode, RuntimeMode::Active)
    }

    pub fn controller_mode(&self, id: ControllerId) -> Option<ControllerMode> {
        self.controllers.get(&id).copied()
    }

    pub fn set_controller_mode(&mut self, id: ControllerId, mode: ControllerMode) {
        self.controllers.insert(id, mode);
    }

    pub fn disconnect_controller(&mut self, id: ControllerId) {
        self.controllers.remove(&id);
    }
}
