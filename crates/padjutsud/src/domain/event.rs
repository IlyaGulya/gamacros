use crate::activity::ActivityEvent;
use crate::api::Command as ApiCommand;
use padjutsu_gamepad::ControllerEvent;
use padjutsu_workspace::ProfileEvent;

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
