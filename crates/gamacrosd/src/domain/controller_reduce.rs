use colored::Colorize;
use gamacros_gamepad::ControllerEvent;

use crate::app::{ButtonPhase, Gamacros};
use crate::domain::reduce::DomainStep;
use crate::domain::{
    push_controller_state_update, resolve_controller_state, ControllerTransition,
    RuntimeState, WakeState, WakeTransition,
};
use crate::print_debug;

pub fn reduce_controller_event(
    controller_event: ControllerEvent,
    step: &mut DomainStep,
    gamacros: &mut Gamacros,
    runtime_state: &RuntimeState,
    wake_state: &WakeState,
    on_ignored: impl FnOnce(&str) -> DomainStep,
) -> Option<DomainStep> {
    match controller_event {
        ControllerEvent::Connected(info) => {
            let id = info.id;
            if gamacros.is_known(id) {
                return None;
            }

            gamacros.add_controller(info);
            push_controller_state_update(
                step,
                runtime_state,
                id,
                resolve_controller_state(gamacros, id),
            );
            step.transition.wake.push(WakeTransition::Reschedule);
        }
        ControllerEvent::Disconnected(id) => {
            gamacros.remove_controller(id);
            gamacros.on_controller_disconnected(id);
            print_debug!("controller state transition: id={id} -> Disconnected");
            step.transition
                .controller_updates
                .push(ControllerTransition {
                    id,
                    next_state: None,
                });
            step.transition.wake.push(WakeTransition::Reschedule);
        }
        ControllerEvent::ButtonPressed { id, button } => {
            if !runtime_state.allows_input_actions() {
                return Some(on_ignored("button press"));
            }
            step.transition.effects =
                gamacros.on_button_effects(id, button, ButtonPhase::Pressed);
            let next_state = resolve_controller_state(gamacros, id);
            push_controller_state_update(step, runtime_state, id, next_state);
            step.transition.wake.push(WakeTransition::Reschedule);
        }
        ControllerEvent::ButtonReleased { id, button } => {
            if !runtime_state.allows_input_actions() {
                return Some(on_ignored("button release"));
            }
            step.transition.effects =
                gamacros.on_button_effects(id, button, ButtonPhase::Released);
            let next_state = resolve_controller_state(gamacros, id);
            push_controller_state_update(step, runtime_state, id, next_state);
            step.transition.wake.push(WakeTransition::Reschedule);
        }
        ControllerEvent::AxisMotion { id, axis, value } => {
            print_debug!(
                "domain event: axis motion controller={id} axis={axis:?} value={value:.3}"
            );
            gamacros.on_axis_motion(id, axis, value);
            let next_state = resolve_controller_state(gamacros, id);
            push_controller_state_update(step, runtime_state, id, next_state);
            if runtime_state.allows_input_actions() && !wake_state.ticking_enabled {
                step.transition.wake.push(WakeTransition::Reschedule);
                print_debug!(
                    "axis motion armed ticking: controller={id} axis={axis:?} value={value:.3}"
                );
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ControllerMode, RuntimeMode};
    use gamacros_gamepad::{Button, ControllerInfo};

    fn controller_info(id: u32) -> ControllerInfo {
        ControllerInfo {
            id,
            name: "Test Controller".into(),
            supports_rumble: false,
            vendor_id: 1,
            product_id: 1,
        }
    }

    #[test]
    fn ignores_button_press_outside_active_runtime() {
        let mut step = DomainStep::continue_();
        let mut gamacros = Gamacros::new();
        gamacros.add_controller(controller_info(1));
        let runtime_state = RuntimeState::new(RuntimeMode::AwaitingProfile);
        let wake_state = WakeState::new(std::time::Instant::now());

        let ignored = reduce_controller_event(
            ControllerEvent::ButtonPressed {
                id: 1,
                button: Button::A,
            },
            &mut step,
            &mut gamacros,
            &runtime_state,
            &wake_state,
            |_| DomainStep::continue_(),
        );

        assert!(ignored.is_some());
        assert!(step.transition.effects.is_empty());
    }

    #[test]
    fn connect_event_enqueues_controller_state_update() {
        let mut step = DomainStep::continue_();
        let mut gamacros = Gamacros::new();
        let runtime_state = RuntimeState::new(RuntimeMode::Active);
        let wake_state = WakeState::new(std::time::Instant::now());

        let ignored = reduce_controller_event(
            ControllerEvent::Connected(controller_info(1)),
            &mut step,
            &mut gamacros,
            &runtime_state,
            &wake_state,
            |_| DomainStep::continue_(),
        );

        assert!(ignored.is_none());
        assert_eq!(step.transition.controller_updates.len(), 1);
        assert!(matches!(
            step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::ConnectedIdle
        ));
    }
}
