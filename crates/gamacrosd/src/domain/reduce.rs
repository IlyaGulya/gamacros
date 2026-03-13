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

#[cfg(test)]
mod tests {
    use ahash::{AHashMap, AHashSet};

    use super::*;
    use crate::domain::{
        ControllerMode, ModeTransition, RuntimeMode, ShellTransition, WakeTransition,
    };
    use gamacros_gamepad::{Button, ControllerEvent, ControllerInfo};
    use gamacros_workspace::{AppRules, Profile, ProfileEvent};

    fn controller_info(id: u32) -> ControllerInfo {
        ControllerInfo {
            id,
            name: "Test Controller".into(),
            supports_rumble: false,
            vendor_id: 1,
            product_id: 1,
        }
    }

    fn profile_with_common_rules() -> Profile {
        let mut rules = AHashMap::new();
        rules.insert("common".into(), AppRules::default());

        Profile {
            controllers: AHashMap::new(),
            blacklist: AHashSet::new(),
            rules,
            shell: Some("/bin/zsh".into()),
        }
    }

    #[test]
    fn reduce_event_profile_changed_emits_runtime_transition() {
        let mut gamacros = Gamacros::new();
        let manager = ControllerManager::new().expect("manager init");
        let runtime_state = RuntimeState::new(RuntimeMode::AwaitingProfile);
        let wake_state = WakeState::new(std::time::Instant::now());

        let step = reduce_event(
            DomainEvent::Profile(ProfileEvent::Changed(profile_with_common_rules())),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert!(matches!(
            step.transition.mode,
            Some(ModeTransition::Set(RuntimeMode::Active))
        ));
        assert!(matches!(
            step.transition.shell,
            Some(ShellTransition::Set(Some(_)))
        ));
        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::Reschedule]
        ));
    }

    #[test]
    fn reduce_event_controller_button_press_updates_controller_state() {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_common_rules());
        gamacros.add_controller(controller_info(7));
        let manager = ControllerManager::new().expect("manager init");
        let mut runtime_state = RuntimeState::new(RuntimeMode::Active);
        runtime_state.set_controller_state(
            7,
            crate::domain::ControllerRuntimeState::new(
                ControllerMode::ConnectedIdle,
                crate::domain::resolve_stick_state(
                    &gamacros,
                    7,
                    gamacros_workspace::StickSide::Left,
                ),
                crate::domain::resolve_stick_state(
                    &gamacros,
                    7,
                    gamacros_workspace::StickSide::Right,
                ),
            ),
        );
        let wake_state = WakeState::new(std::time::Instant::now());

        let step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonPressed {
                id: 7,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert_eq!(step.transition.controller_updates.len(), 1);
        assert!(matches!(
            step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::ButtonsActive
        ));
        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::Reschedule, ..]
        ));
    }

    #[test]
    fn reduce_event_timer_wake_outside_active_runtime_only_disables_fast_mode() {
        let mut gamacros = Gamacros::new();
        let manager = ControllerManager::new().expect("manager init");
        let runtime_state = RuntimeState::new(RuntimeMode::AwaitingProfile);
        let mut wake_state = WakeState::new(std::time::Instant::now());
        wake_state.fast_mode = true;

        let step = reduce_event(
            DomainEvent::Timer(crate::domain::TimerEvent::Wake),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert!(step.transition.effects.is_empty());
        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::DisableFastMode]
        ));
    }
}
