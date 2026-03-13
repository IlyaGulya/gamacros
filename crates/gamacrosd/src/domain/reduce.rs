use std::time::Duration;

use colored::Colorize;
use gamacros_gamepad::{ControllerEvent, ControllerManager};
use gamacros_workspace::ProfileEvent;

use crate::activity::ActivityEvent;
use crate::api::Command as ApiCommand;
use crate::app::{ButtonPhase, Effect, Gamacros};
use crate::domain::{
    resolve_stick_state, stick_transition, ControllerMode, ControllerRuntimeState,
    DomainEvent, RuntimeMode, RuntimeState, StickActivity, SystemEvent, TimerEvent,
    WakeIntent, WakeState,
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

fn resolve_controller_mode(
    gamacros: &Gamacros,
    id: gamacros_gamepad::ControllerId,
) -> ControllerMode {
    let has_buttons = gamacros.controller_has_pressed_buttons(id);
    let left_stick =
        resolve_stick_state(gamacros, id, gamacros_workspace::StickSide::Left);
    let right_stick =
        resolve_stick_state(gamacros, id, gamacros_workspace::StickSide::Right);
    let has_stick_activity = matches!(
        left_stick.activity,
        StickActivity::Active | StickActivity::Repeating
    ) || matches!(
        right_stick.activity,
        StickActivity::Active | StickActivity::Repeating
    );
    let has_stick_repeats = matches!(left_stick.activity, StickActivity::Repeating)
        || matches!(right_stick.activity, StickActivity::Repeating);
    let has_repeats = gamacros.controller_has_repeats(id) || has_stick_repeats;

    print_debug!(
        "controller state inputs: id={id} buttons={} left_stick={:?}/{:?} right_stick={:?}/{:?} repeats={}",
        has_buttons,
        left_stick.mode,
        left_stick.activity,
        right_stick.mode,
        right_stick.activity,
        has_repeats
    );

    match (has_buttons, has_stick_activity, has_repeats) {
        (_, _, true) if has_buttons || has_stick_activity => {
            ControllerMode::RepeatingWithInput
        }
        (_, _, true) => ControllerMode::Repeating,
        (false, false, false) => ControllerMode::ConnectedIdle,
        (true, false, false) => ControllerMode::ButtonsActive,
        (false, true, false) => ControllerMode::AxisActive,
        (true, true, false) => ControllerMode::MixedInput,
    }
}

fn push_controller_mode_update(
    step: &mut DomainStep,
    runtime_state: &RuntimeState,
    id: gamacros_gamepad::ControllerId,
    next_state: ControllerRuntimeState,
) {
    let previous_state = runtime_state.controller_state(id);
    if previous_state != Some(next_state) {
        if let Some((prev, next)) = stick_transition(
            previous_state.map(|state| state.left_stick()),
            next_state.left_stick(),
        ) {
            print_debug!(
                "left stick transition: controller={id} prev={prev:?} next={next:?}"
            );
        }
        if let Some((prev, next)) = stick_transition(
            previous_state.map(|state| state.right_stick()),
            next_state.right_stick(),
        ) {
            print_debug!(
                "right stick transition: controller={id} prev={prev:?} next={next:?}"
            );
        }
        print_debug!(
            "controller state transition: id={id} prev={:?} -> next={:?} left={:?}/{:?} right={:?}/{:?}",
            previous_state.map(|state| state.mode()),
            next_state.mode(),
            next_state.left_stick().mode,
            next_state.left_stick().activity,
            next_state.right_stick().mode,
            next_state.right_stick().activity,
        );
    }
    step.controller_updates.push((id, Some(next_state)));
}

fn resolve_controller_state(
    gamacros: &Gamacros,
    id: gamacros_gamepad::ControllerId,
) -> ControllerRuntimeState {
    let left_stick =
        resolve_stick_state(gamacros, id, gamacros_workspace::StickSide::Left);
    let right_stick =
        resolve_stick_state(gamacros, id, gamacros_workspace::StickSide::Right);
    let mode = resolve_controller_mode(gamacros, id);
    ControllerRuntimeState::new(mode, left_stick, right_stick)
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
        DomainEvent::Controller(controller_event) => match controller_event {
            ControllerEvent::Connected(info) => {
                let id = info.id;
                if gamacros.is_known(id) {
                    return step;
                }

                gamacros.add_controller(info);
                push_controller_mode_update(
                    &mut step,
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
                    return ignored_for_mode(runtime_state, "button press");
                }
                step.effects =
                    gamacros.on_button_effects(id, button, ButtonPhase::Pressed);
                let next_state = resolve_controller_state(gamacros, id);
                push_controller_mode_update(
                    &mut step,
                    runtime_state,
                    id,
                    next_state,
                );
                step.wake_intents.push(WakeIntent::Reschedule);
            }
            ControllerEvent::ButtonReleased { id, button } => {
                if !runtime_state.allows_input_actions() {
                    return ignored_for_mode(runtime_state, "button release");
                }
                step.effects =
                    gamacros.on_button_effects(id, button, ButtonPhase::Released);
                let next_state = resolve_controller_state(gamacros, id);
                push_controller_mode_update(
                    &mut step,
                    runtime_state,
                    id,
                    next_state,
                );
                step.wake_intents.push(WakeIntent::Reschedule);
            }
            ControllerEvent::AxisMotion { id, axis, value } => {
                print_debug!(
                    "domain event: axis motion controller={id} axis={axis:?} value={value:.3}"
                );
                gamacros.on_axis_motion(id, axis, value);
                let next_state = resolve_controller_state(gamacros, id);
                push_controller_mode_update(
                    &mut step,
                    runtime_state,
                    id,
                    next_state,
                );
                if runtime_state.allows_input_actions()
                    && !wake_state.ticking_enabled
                {
                    step.wake_intents.push(WakeIntent::Reschedule);
                    print_debug!(
                        "axis motion armed ticking: controller={id} axis={axis:?} value={value:.3}"
                    );
                }
            }
        },
        DomainEvent::Activity(activity_event) => {
            let ActivityEvent::DidActivateApplication(bundle_id) = activity_event
            else {
                return step;
            };
            gamacros.set_active_app(&bundle_id);
            step.set_shell = Some(gamacros.current_shell());
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        DomainEvent::Profile(profile_event) => match profile_event {
            ProfileEvent::Changed(workspace) => {
                print_info!("profile changed, updating workspace");
                gamacros.set_workspace(workspace);
                step.set_shell = Some(gamacros.current_shell());
                step.wake_intents.push(WakeIntent::Reschedule);
                transition_to_active(&mut step);
            }
            ProfileEvent::Removed => {
                gamacros.remove_workspace();
                step.set_shell = Some(gamacros.current_shell());
                step.wake_intents.push(WakeIntent::Reschedule);
                transition_to_awaiting_profile(&mut step);
            }
            ProfileEvent::Error(error) => {
                print_error!("profile error: {error}");
            }
        },
        DomainEvent::Api(command) => match command {
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
        },
        DomainEvent::Timer(TimerEvent::Wake) => {
            if !runtime_state.handles_timer_wake() {
                if wake_state.fast_mode {
                    step.wake_intents.push(WakeIntent::DisableFastMode);
                }
                print_debug!(
                    "ignoring timer wake while runtime mode is {:?}",
                    runtime_state.mode()
                );
                return step;
            }
            let now = std::time::Instant::now();
            print_debug!(
                "wake timer fired: next_tick_due={:?} fast_mode={} need_reschedule={}",
                wake_state.next_tick_due,
                wake_state.fast_mode,
                wake_state.need_reschedule
            );
            if let Some(due) = wake_state.next_tick_due {
                if now >= due {
                    print_debug!(
                        "wake timer: tick due lateness_us={}",
                        now.duration_since(due).as_micros()
                    );
                    step.effects.extend(gamacros.on_tick_effects());
                    if gamacros.wants_fast_tick() {
                        let fast_until = now + Duration::from_millis(250);
                        step.wake_intents
                            .push(WakeIntent::EnableFastModeUntil(fast_until));
                        print_debug!(
                            "wake timer: enabling fast mode until {fast_until:?}"
                        );
                    } else if wake_state.fast_mode && now >= wake_state.fast_until {
                        step.wake_intents.push(WakeIntent::DisableFastMode);
                        print_debug!("wake timer: disabling fast mode");
                    }
                }
            }
            let repeats_started_at = std::time::Instant::now();
            step.effects.extend(gamacros.due_repeat_effects(now));
            print_debug!(
                "wake timer: stick repeats elapsed_us={}",
                repeats_started_at.elapsed().as_micros()
            );
            let button_repeats_started_at = std::time::Instant::now();
            step.effects.extend(gamacros.button_repeat_effects(now));
            print_debug!(
                "wake timer: button repeats elapsed_us={}",
                button_repeats_started_at.elapsed().as_micros()
            );
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        DomainEvent::System(SystemEvent::ShutdownRequested) => {
            return transition_to_shutting_down(DomainStep::break_());
        }
    }

    step
}
