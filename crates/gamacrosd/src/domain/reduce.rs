use colored::Colorize;
use gamacros_gamepad::{ControllerEvent, ControllerManager};
use gamacros_workspace::ProfileEvent;

use crate::activity::ActivityEvent;
use crate::api::Command as ApiCommand;
use crate::app::{ButtonPhase, Effect, Gamacros};
use crate::domain::{
    ControllerRuntimeState, push_controller_state_update, resolve_controller_state,
    reduce_timer_event, DomainEvent, RuntimeMode, RuntimeState, SystemEvent, WakeIntent, WakeState,
};
use crate::{print_debug, print_error, print_info};

pub enum DomainControl {
    Continue,
    Break,
}

pub struct DomainStep {
    pub effects: Vec<Effect>,
    pub set_shell: Option<Option<Box<str>>>,
    pub wake_intents: Vec<WakeIntent>,
    pub controller_updates: Vec<(
        gamacros_gamepad::ControllerId,
        Option<ControllerRuntimeState>,
    )>,
    pub next_mode: Option<RuntimeMode>,
    pub control: DomainControl,
}

impl DomainStep {
    pub fn continue_() -> Self {
        Self {
            effects: Vec::new(),
            set_shell: None,
            wake_intents: Vec::new(),
            controller_updates: Vec::new(),
            next_mode: None,
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
        self.next_mode = Some(mode);
        self
    }
}

fn transition_to_active(step: &mut DomainStep) {
    step.next_mode = Some(RuntimeMode::Active);
}

fn transition_to_awaiting_profile(step: &mut DomainStep) {
    step.next_mode = Some(RuntimeMode::AwaitingProfile);
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

fn reduce_controller_event(
    controller_event: ControllerEvent,
    step: &mut DomainStep,
    gamacros: &mut Gamacros,
    runtime_state: &RuntimeState,
    wake_state: &WakeState,
) {
    match controller_event {
        ControllerEvent::Connected(info) => {
            let id = info.id;
            if gamacros.is_known(id) {
                return;
            }

            gamacros.add_controller(info);
            push_controller_state_update(
                step,
                runtime_state,
                id,
                resolve_controller_state(gamacros, id),
            );
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        ControllerEvent::Disconnected(id) => {
            gamacros.remove_controller(id);
            gamacros.on_controller_disconnected(id);
            print_debug!("controller state transition: id={id} -> Disconnected");
            step.controller_updates.push((id, None));
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        ControllerEvent::ButtonPressed { id, button } => {
            if !runtime_state.allows_input_actions() {
                *step = ignored_for_mode(runtime_state, "button press");
                return;
            }
            step.effects =
                gamacros.on_button_effects(id, button, ButtonPhase::Pressed);
            let next_state = resolve_controller_state(gamacros, id);
            push_controller_state_update(step, runtime_state, id, next_state);
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        ControllerEvent::ButtonReleased { id, button } => {
            if !runtime_state.allows_input_actions() {
                *step = ignored_for_mode(runtime_state, "button release");
                return;
            }
            step.effects =
                gamacros.on_button_effects(id, button, ButtonPhase::Released);
            let next_state = resolve_controller_state(gamacros, id);
            push_controller_state_update(step, runtime_state, id, next_state);
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        ControllerEvent::AxisMotion { id, axis, value } => {
            print_debug!(
                "domain event: axis motion controller={id} axis={axis:?} value={value:.3}"
            );
            gamacros.on_axis_motion(id, axis, value);
            let next_state = resolve_controller_state(gamacros, id);
            push_controller_state_update(step, runtime_state, id, next_state);
            if runtime_state.allows_input_actions() && !wake_state.ticking_enabled {
                step.wake_intents.push(WakeIntent::Reschedule);
                print_debug!(
                    "axis motion armed ticking: controller={id} axis={axis:?} value={value:.3}"
                );
            }
        }
    }
}

fn reduce_activity_event(
    activity_event: ActivityEvent,
    step: &mut DomainStep,
    gamacros: &mut Gamacros,
) {
    let ActivityEvent::DidActivateApplication(bundle_id) = activity_event else {
        return;
    };
    gamacros.set_active_app(&bundle_id);
    step.set_shell = Some(gamacros.current_shell());
    step.wake_intents.push(WakeIntent::Reschedule);
}

fn reduce_profile_event(
    profile_event: ProfileEvent,
    step: &mut DomainStep,
    gamacros: &mut Gamacros,
) {
    match profile_event {
        ProfileEvent::Changed(workspace) => {
            print_info!("profile changed, updating workspace");
            gamacros.set_workspace(workspace);
            step.set_shell = Some(gamacros.current_shell());
            step.wake_intents.push(WakeIntent::Reschedule);
            transition_to_active(step);
        }
        ProfileEvent::Removed => {
            gamacros.remove_workspace();
            step.set_shell = Some(gamacros.current_shell());
            step.wake_intents.push(WakeIntent::Reschedule);
            transition_to_awaiting_profile(step);
        }
        ProfileEvent::Error(error) => {
            print_error!("profile error: {error}");
        }
    }
}

fn reduce_api_command(
    command: ApiCommand,
    step: &mut DomainStep,
    manager: &ControllerManager,
) {
    match command {
        ApiCommand::Rumble { id, ms } => match id {
            Some(controller_id) => {
                step.effects.push(Effect::Rumble {
                    id: controller_id,
                    ms,
                });
            }
            None => {
                for info in manager.controllers() {
                    step.effects.push(Effect::Rumble { id: info.id, ms });
                }
            }
        },
    }
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
            reduce_controller_event(
                controller_event,
                &mut step,
                gamacros,
                runtime_state,
                wake_state,
            );
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
