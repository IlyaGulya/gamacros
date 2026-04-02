use ahash::AHashMap;
use padjutsu_gamepad::ControllerId;

use crate::domain::stick_state::StickState;

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
    Repeating,
    RepeatingWithInput,
}

pub struct RuntimeState {
    mode: RuntimeMode,
    controllers: AHashMap<ControllerId, ControllerRuntimeState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerRuntimeState {
    mode: ControllerMode,
    left_stick: StickState,
    right_stick: StickState,
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

    pub fn controller_state(
        &self,
        id: ControllerId,
    ) -> Option<ControllerRuntimeState> {
        self.controllers.get(&id).copied()
    }

    pub fn set_controller_state(
        &mut self,
        id: ControllerId,
        state: ControllerRuntimeState,
    ) {
        self.controllers.insert(id, state);
    }

    pub fn disconnect_controller(&mut self, id: ControllerId) {
        self.controllers.remove(&id);
    }
}

impl ControllerRuntimeState {
    pub fn new(
        mode: ControllerMode,
        left_stick: StickState,
        right_stick: StickState,
    ) -> Self {
        Self {
            mode,
            left_stick,
            right_stick,
        }
    }

    pub fn mode(&self) -> ControllerMode {
        self.mode
    }

    pub fn left_stick(&self) -> StickState {
        self.left_stick
    }

    pub fn right_stick(&self) -> StickState {
        self.right_stick
    }
}
