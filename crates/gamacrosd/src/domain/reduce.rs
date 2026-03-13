use colored::Colorize;
use gamacros_gamepad::ControllerManager;
use crate::app::Gamacros;
use crate::domain::{
    reduce_activity_event, reduce_api_command, reduce_controller_event,
    reduce_profile_event, reduce_timer_event, DomainEvent, RuntimeMode,
    RuntimeState, SystemEvent, Transition, WakeState,
};
use crate::print_debug;

pub enum DomainControl {
    Continue,
    Break,
}

pub struct DomainStep {
    pub transition: Transition,
    pub control: DomainControl,
}

impl DomainStep {
    pub fn continue_() -> Self {
        Self {
            transition: Transition::new(),
            control: DomainControl::Continue,
        }
    }

    pub fn break_() -> Self {
        Self {
            control: DomainControl::Break,
            ..Self::continue_()
        }
    }

    fn with_mode(mut self, mode: RuntimeMode) -> Self {
        self.transition.mode = Some(crate::domain::ModeTransition::Set(mode));
        self
    }
}

fn transition_to_shutting_down(step: DomainStep) -> DomainStep {
    step.with_mode(RuntimeMode::ShuttingDown)
}

fn ignored_for_mode(runtime_state: &RuntimeState, what: &str) -> DomainStep {
    print_debug!(
        "ignoring {what} while runtime mode is {:?}",
        runtime_state.mode()
    );
    DomainStep::continue_()
}

pub fn reduce_event(
    event: DomainEvent,
    gamacros: &mut Gamacros,
    manager: &ControllerManager,
    runtime_state: &RuntimeState,
    wake_state: &WakeState,
) -> DomainStep {
    let mut step = DomainStep::continue_();

    if runtime_state.mode() == RuntimeMode::ShuttingDown {
        if matches!(event, DomainEvent::System(SystemEvent::ShutdownRequested)) {
            return transition_to_shutting_down(DomainStep::break_());
        }
        return step;
    }

    match event {
        DomainEvent::Controller(controller_event) => {
            if let Some(ignored_step) = reduce_controller_event(
                controller_event,
                &mut step,
                gamacros,
                runtime_state,
                wake_state,
                |what| ignored_for_mode(runtime_state, what),
            ) {
                return ignored_step;
            }
        }
        DomainEvent::Activity(activity_event) => {
            reduce_activity_event(activity_event, &mut step, gamacros);
        }
        DomainEvent::Profile(profile_event) => {
            reduce_profile_event(profile_event, &mut step, gamacros);
        }
        DomainEvent::Api(command) => {
            reduce_api_command(command, &mut step, manager);
        }
        DomainEvent::Timer(timer_event) => {
            reduce_timer_event(
                timer_event,
                &mut step,
                gamacros,
                runtime_state,
                wake_state,
            );
        }
        DomainEvent::System(SystemEvent::ShutdownRequested) => {
            return transition_to_shutting_down(DomainStep::break_());
        }
    }

    step
}
