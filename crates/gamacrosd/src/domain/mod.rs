mod activity_reduce;
mod api_reduce;
mod controller_reduce;
mod event;
mod profile_reduce;
mod reduce;
mod stick_reduce;
mod stick_state;
mod state;
mod timer_reduce;
mod transition;
mod wake;

pub use activity_reduce::reduce_activity_event;
pub use api_reduce::reduce_api_command;
pub use controller_reduce::reduce_controller_event;
pub use event::{DomainEvent, SystemEvent, TimerEvent};
pub use profile_reduce::reduce_profile_event;
pub use reduce::{reduce_event, DomainControl, DomainStep};
pub use stick_reduce::{push_controller_state_update, resolve_controller_state};
pub use stick_state::{resolve_stick_state, stick_transition, StickActivity};
pub use state::{ControllerMode, ControllerRuntimeState, RuntimeMode, RuntimeState};
pub use timer_reduce::reduce_timer_event;
pub use transition::{
    ControllerTransition, ModeTransition, ShellTransition, Transition,
    WakeTransition,
};
pub use wake::{apply_wake_intents, overdue_wake_event, reschedule_wake, WakeState};
