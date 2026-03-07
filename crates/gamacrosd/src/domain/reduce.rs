use std::time::Duration;

use colored::Colorize;
use gamacros_gamepad::{ControllerEvent, ControllerManager};
use gamacros_workspace::ProfileEvent;

use crate::activity::ActivityEvent;
use crate::api::Command as ApiCommand;
use crate::app::{ButtonPhase, Effect, Gamacros};
use crate::domain::{
    DomainEvent, RuntimeMode, RuntimeState, SystemEvent, TimerEvent, WakeIntent,
    WakeState,
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
    pub next_mode: Option<RuntimeMode>,
    pub control: DomainControl,
}

impl DomainStep {
    pub fn continue_() -> Self {
        Self {
            effects: Vec::new(),
            set_shell: None,
            wake_intents: Vec::new(),
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
}

pub fn reduce_event(
    event: DomainEvent,
    gamacros: &mut Gamacros,
    manager: &ControllerManager,
    runtime_state: &RuntimeState,
    wake_state: &WakeState,
) -> DomainStep {
    let mut step = DomainStep::continue_();
    let is_active = runtime_state.is_active();

    if runtime_state.mode() == RuntimeMode::ShuttingDown {
        if matches!(event, DomainEvent::System(SystemEvent::ShutdownRequested)) {
            return DomainStep::break_();
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
                step.wake_intents.push(WakeIntent::Reschedule);
            }
            ControllerEvent::Disconnected(id) => {
                gamacros.remove_controller(id);
                gamacros.on_controller_disconnected(id);
                step.wake_intents.push(WakeIntent::Reschedule);
            }
            ControllerEvent::ButtonPressed { id, button } => {
                if !is_active {
                    print_debug!(
                        "ignoring button press while runtime mode is {:?}",
                        runtime_state.mode()
                    );
                    return step;
                }
                step.effects =
                    gamacros.on_button_effects(id, button, ButtonPhase::Pressed);
                step.wake_intents.push(WakeIntent::Reschedule);
            }
            ControllerEvent::ButtonReleased { id, button } => {
                if !is_active {
                    print_debug!(
                        "ignoring button release while runtime mode is {:?}",
                        runtime_state.mode()
                    );
                    return step;
                }
                step.effects =
                    gamacros.on_button_effects(id, button, ButtonPhase::Released);
                step.wake_intents.push(WakeIntent::Reschedule);
            }
            ControllerEvent::AxisMotion { id, axis, value } => {
                print_debug!(
                    "domain event: axis motion controller={id} axis={axis:?} value={value:.3}"
                );
                gamacros.on_axis_motion(id, axis, value);
                if is_active && !wake_state.ticking_enabled {
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
                step.next_mode = Some(RuntimeMode::Active);
            }
            ProfileEvent::Removed => {
                gamacros.remove_workspace();
                step.set_shell = Some(gamacros.current_shell());
                step.wake_intents.push(WakeIntent::Reschedule);
                step.next_mode = Some(RuntimeMode::AwaitingProfile);
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
            if !is_active {
                if wake_state.fast_mode {
                    step.wake_intents.push(WakeIntent::DisableFastMode);
                }
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
            step.next_mode = Some(RuntimeMode::ShuttingDown);
            return DomainStep::break_();
        }
    }

    step
}
