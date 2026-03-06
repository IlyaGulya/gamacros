use crate::activity::ActivityEvent;
use crate::api::Command as ApiCommand;
use gamacros_gamepad::ControllerEvent;
use gamacros_workspace::ProfileEvent;

pub enum DomainEvent {
    Controller(ControllerEvent),
    Activity(ActivityEvent),
    Profile(ProfileEvent),
    Api(ApiCommand),
    Timer(TimerEvent),
    System(SystemEvent),
}

pub enum TimerEvent {
    Wake,
}

pub enum SystemEvent {
    ShutdownRequested,
}
