#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Booting,
    AwaitingProfile,
    Active,
    ShuttingDown,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeState {
    mode: RuntimeMode,
}

impl RuntimeState {
    pub fn new(mode: RuntimeMode) -> Self {
        Self { mode }
    }

    pub fn mode(&self) -> RuntimeMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: RuntimeMode) {
        self.mode = mode;
    }

    pub fn is_active(&self) -> bool {
        self.mode == RuntimeMode::Active
    }

    pub fn allows_input_actions(&self) -> bool {
        matches!(self.mode, RuntimeMode::Active)
    }

    pub fn handles_timer_wake(&self) -> bool {
        matches!(self.mode, RuntimeMode::Active)
    }
}
