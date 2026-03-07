mod event;
mod wake;

pub use event::{DomainEvent, SystemEvent, TimerEvent};
pub use wake::{has_overdue_work, reschedule_wake, WakeState};
