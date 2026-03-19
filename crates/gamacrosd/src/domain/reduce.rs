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
    use std::sync::Arc;

    use ahash::{AHashMap, AHashSet};

    use super::*;
    use crate::domain::{
        ControllerMode, ModeTransition, RuntimeMode, ShellTransition, WakeTransition,
    };
    use gamacros_bit_mask::Bitmask;
    use gamacros_control::{Key, KeyCombo};
    use gamacros_gamepad::{Button, ControllerEvent, ControllerInfo};
    use gamacros_workspace::{AppRules, ButtonAction, ButtonRule, Profile, ProfileEvent};

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

    fn profile_with_repeating_button_rule() -> Profile {
        let mut buttons = AHashMap::new();
        buttons.insert(
            Bitmask::new(&[Button::A]),
            ButtonRule {
                action: ButtonAction::Keystroke(Arc::new(KeyCombo::from_key(
                    Key::Unicode('a'),
                ))),
                vibrate: None,
                repeat_delay_ms: Some(1),
                repeat_interval_ms: Some(1),
            },
        );

        let mut rules = AHashMap::new();
        rules.insert(
            "common".into(),
            AppRules {
                buttons,
                ..AppRules::default()
            },
        );

        Profile {
            controllers: AHashMap::new(),
            blacklist: AHashSet::new(),
            rules,
            shell: Some("/bin/zsh".into()),
        }
    }

    fn profile_with_hold_button_rule() -> Profile {
        let mut buttons = AHashMap::new();
        buttons.insert(
            Bitmask::new(&[Button::A]),
            ButtonRule {
                action: ButtonAction::HoldKeystroke(Arc::new(KeyCombo::from_key(
                    Key::Unicode('a'),
                ))),
                vibrate: None,
                repeat_delay_ms: None,
                repeat_interval_ms: None,
            },
        );

        let mut rules = AHashMap::new();
        rules.insert(
            "common".into(),
            AppRules {
                buttons,
                ..AppRules::default()
            },
        );

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

    #[test]
    fn reduce_event_shutdown_requested_breaks_and_sets_shutting_down_mode() {
        let mut gamacros = Gamacros::new();
        let manager = ControllerManager::new().expect("manager init");
        let runtime_state = RuntimeState::new(RuntimeMode::Active);
        let wake_state = WakeState::new(std::time::Instant::now());

        let step = reduce_event(
            DomainEvent::System(SystemEvent::ShutdownRequested),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert!(matches!(step.control, DomainControl::Break));
        assert!(matches!(
            step.transition.mode,
            Some(ModeTransition::Set(RuntimeMode::ShuttingDown))
        ));
    }

    #[test]
    fn reduce_event_ignores_controller_input_while_shutting_down() {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_common_rules());
        gamacros.add_controller(controller_info(7));
        let manager = ControllerManager::new().expect("manager init");
        let runtime_state = RuntimeState::new(RuntimeMode::ShuttingDown);
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

        assert!(matches!(step.control, DomainControl::Continue));
        assert!(step.transition.effects.is_empty());
        assert!(step.transition.controller_updates.is_empty());
        assert!(step.transition.wake.is_empty());
    }

    #[test]
    fn reduce_event_activity_then_profile_then_controller_forms_expected_trace() {
        let mut gamacros = Gamacros::new();
        let manager = ControllerManager::new().expect("manager init");
        let wake_state = WakeState::new(std::time::Instant::now());
        let mut runtime_state = RuntimeState::new(RuntimeMode::AwaitingProfile);

        let activity_step = reduce_event(
            DomainEvent::Activity(
                crate::activity::ActivityEvent::DidActivateApplication(
                    "ru.keepcoder.Telegram".into(),
                ),
            ),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        assert!(matches!(
            activity_step.transition.shell,
            Some(ShellTransition::Set(_))
        ));

        let profile_step = reduce_event(
            DomainEvent::Profile(ProfileEvent::Changed(profile_with_common_rules())),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        assert!(matches!(
            profile_step.transition.mode,
            Some(ModeTransition::Set(RuntimeMode::Active))
        ));

        runtime_state.set_mode(RuntimeMode::Active);
        gamacros.set_workspace(profile_with_common_rules());
        gamacros.add_controller(controller_info(42));
        runtime_state.set_controller_state(
            42,
            crate::domain::ControllerRuntimeState::new(
                ControllerMode::ConnectedIdle,
                crate::domain::resolve_stick_state(
                    &gamacros,
                    42,
                    gamacros_workspace::StickSide::Left,
                ),
                crate::domain::resolve_stick_state(
                    &gamacros,
                    42,
                    gamacros_workspace::StickSide::Right,
                ),
            ),
        );

        let controller_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonPressed {
                id: 42,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert_eq!(controller_step.transition.controller_updates.len(), 1);
        assert!(matches!(
            controller_step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::ButtonsActive
        ));
    }

    #[test]
    fn reduce_event_connect_then_disconnect_forms_expected_lifecycle_trace() {
        let mut gamacros = Gamacros::new();
        let manager = ControllerManager::new().expect("manager init");
        let wake_state = WakeState::new(std::time::Instant::now());
        let runtime_state = RuntimeState::new(RuntimeMode::Active);

        let connect_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::Connected(controller_info(9))),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert_eq!(connect_step.transition.controller_updates.len(), 1);
        assert!(matches!(
            connect_step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::ConnectedIdle
        ));

        let disconnect_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::Disconnected(9)),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert_eq!(disconnect_step.transition.controller_updates.len(), 1);
        assert!(disconnect_step.transition.controller_updates[0]
            .next_state
            .is_none());
    }

    #[test]
    fn reduce_event_button_repeat_trace_enters_and_exits_repeating_state() {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_repeating_button_rule());
        gamacros.add_controller(controller_info(11));
        let manager = ControllerManager::new().expect("manager init");
        let wake_state = WakeState::new(std::time::Instant::now());
        let mut runtime_state = RuntimeState::new(RuntimeMode::Active);
        runtime_state.set_controller_state(
            11,
            crate::domain::ControllerRuntimeState::new(
                ControllerMode::ConnectedIdle,
                crate::domain::resolve_stick_state(
                    &gamacros,
                    11,
                    gamacros_workspace::StickSide::Left,
                ),
                crate::domain::resolve_stick_state(
                    &gamacros,
                    11,
                    gamacros_workspace::StickSide::Right,
                ),
            ),
        );

        let press_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonPressed {
                id: 11,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        assert!(matches!(
            press_step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::RepeatingWithInput
        ));

        runtime_state.set_controller_state(
            11,
            press_step.transition.controller_updates[0]
                .next_state
                .expect("next state after press"),
        );

        let release_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonReleased {
                id: 11,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        assert!(matches!(
            release_step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::ConnectedIdle
        ));
    }

    #[test]
    fn reduce_event_hold_button_emits_press_then_release_effects() {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_hold_button_rule());
        gamacros.add_controller(controller_info(21));
        let manager = ControllerManager::new().expect("manager init");
        let mut runtime_state = RuntimeState::new(RuntimeMode::Active);
        runtime_state.set_controller_state(
            21,
            crate::domain::ControllerRuntimeState::new(
                ControllerMode::ConnectedIdle,
                crate::domain::resolve_stick_state(
                    &gamacros,
                    21,
                    gamacros_workspace::StickSide::Left,
                ),
                crate::domain::resolve_stick_state(
                    &gamacros,
                    21,
                    gamacros_workspace::StickSide::Right,
                ),
            ),
        );
        let wake_state = WakeState::new(std::time::Instant::now());

        let press_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonPressed {
                id: 21,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        assert!(matches!(
            press_step.transition.effects.as_slice(),
            [crate::app::Effect::KeyPress(_)]
        ));

        let release_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonReleased {
                id: 21,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        assert!(matches!(
            release_step.transition.effects.as_slice(),
            [crate::app::Effect::KeyRelease(_)]
        ));
    }

    #[test]
    fn reduce_event_profile_removed_after_active_runtime_returns_to_awaiting_profile(
    ) {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_common_rules());
        let manager = ControllerManager::new().expect("manager init");
        let runtime_state = RuntimeState::new(RuntimeMode::Active);
        let wake_state = WakeState::new(std::time::Instant::now());

        let step = reduce_event(
            DomainEvent::Profile(ProfileEvent::Removed),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert!(matches!(
            step.transition.mode,
            Some(ModeTransition::Set(RuntimeMode::AwaitingProfile))
        ));
        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::Reschedule]
        ));
    }

    #[test]
    fn reduce_event_axis_then_button_produces_mixed_input_state() {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_common_rules());
        gamacros.add_controller(controller_info(5));
        let manager = ControllerManager::new().expect("manager init");
        let wake_state = WakeState::new(std::time::Instant::now());
        let mut runtime_state = RuntimeState::new(RuntimeMode::Active);
        runtime_state.set_controller_state(
            5,
            crate::domain::ControllerRuntimeState::new(
                ControllerMode::ConnectedIdle,
                crate::domain::resolve_stick_state(
                    &gamacros,
                    5,
                    gamacros_workspace::StickSide::Left,
                ),
                crate::domain::resolve_stick_state(
                    &gamacros,
                    5,
                    gamacros_workspace::StickSide::Right,
                ),
            ),
        );

        let axis_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::AxisMotion {
                id: 5,
                axis: gamacros_gamepad::Axis::LeftX,
                value: 0.9,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );
        runtime_state.set_controller_state(
            5,
            axis_step.transition.controller_updates[0]
                .next_state
                .expect("axis state"),
        );

        let button_step = reduce_event(
            DomainEvent::Controller(ControllerEvent::ButtonPressed {
                id: 5,
                button: Button::A,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert!(matches!(
            button_step.transition.controller_updates[0].next_state,
            Some(state) if state.mode() == ControllerMode::MixedInput
        ));
    }

    #[test]
    fn reduce_event_axis_motion_records_explicit_stick_transition() {
        let mut gamacros = Gamacros::new();
        gamacros.set_workspace(profile_with_common_rules());
        gamacros.add_controller(controller_info(13));
        let manager = ControllerManager::new().expect("manager init");
        let mut runtime_state = RuntimeState::new(RuntimeMode::Active);
        runtime_state.set_controller_state(
            13,
            crate::domain::ControllerRuntimeState::new(
                ControllerMode::ConnectedIdle,
                crate::domain::resolve_stick_state(
                    &gamacros,
                    13,
                    gamacros_workspace::StickSide::Left,
                ),
                crate::domain::resolve_stick_state(
                    &gamacros,
                    13,
                    gamacros_workspace::StickSide::Right,
                ),
            ),
        );
        let wake_state = WakeState::new(std::time::Instant::now());

        let step = reduce_event(
            DomainEvent::Controller(ControllerEvent::AxisMotion {
                id: 13,
                axis: gamacros_gamepad::Axis::LeftX,
                value: 0.9,
            }),
            &mut gamacros,
            &manager,
            &runtime_state,
            &wake_state,
        );

        assert_eq!(step.transition.stick_updates.len(), 1);
        assert!(matches!(
            step.transition.stick_updates[0].previous,
            Some(prev) if prev.activity == crate::domain::StickActivity::Neutral
        ));
        assert!(matches!(
            step.transition.stick_updates[0].next.activity,
            crate::domain::StickActivity::Active
        ));
    }
}
