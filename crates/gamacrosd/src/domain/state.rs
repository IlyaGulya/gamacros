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
}
