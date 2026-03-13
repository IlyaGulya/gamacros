use colored::Colorize;

use crate::app::Gamacros;
use crate::domain::reduce::DomainStep;
use crate::domain::stick_state::StickState;
use crate::domain::{
    resolve_stick_state, stick_transition, ControllerMode, ControllerRuntimeState,
    RuntimeState, StickActivity, WakeIntent,
};
use crate::print_debug;

fn apply_stick_transition_intents(
    step: &mut DomainStep,
    controller_id: gamacros_gamepad::ControllerId,
    previous: Option<StickState>,
    next: StickState,
) {
    let Some((prev, next)) = stick_transition(previous, next) else {
        return;
    };

    match (prev.map(|state| state.activity), next.activity) {
        (
            Some(crate::domain::StickActivity::Neutral),
            crate::domain::StickActivity::Active,
        )
        | (
            Some(crate::domain::StickActivity::Neutral),
            crate::domain::StickActivity::Repeating,
        ) => {
            print_debug!(
                "stick activation intent: controller={controller_id} side={:?} mode={:?}",
                next.side,
                next.mode
            );
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        (
            Some(crate::domain::StickActivity::Active),
            crate::domain::StickActivity::Repeating,
        ) => {
            print_debug!(
                "stick repeating intent: controller={controller_id} side={:?} mode={:?}",
                next.side,
                next.mode
            );
            step.wake_intents.push(WakeIntent::Reschedule);
        }
        (_, crate::domain::StickActivity::Neutral) => {
            print_debug!(
                "stick neutral intent: controller={controller_id} side={:?}",
                next.side
            );
        }
        _ => {}
    }
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

pub fn resolve_controller_state(
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

pub fn push_controller_state_update(
    step: &mut DomainStep,
    runtime_state: &RuntimeState,
    id: gamacros_gamepad::ControllerId,
    next_state: ControllerRuntimeState,
) {
    let previous_state = runtime_state.controller_state(id);
    if previous_state != Some(next_state) {
        apply_stick_transition_intents(
            step,
            id,
            previous_state.map(|state| state.left_stick()),
            next_state.left_stick(),
        );
        apply_stick_transition_intents(
            step,
            id,
            previous_state.map(|state| state.right_stick()),
            next_state.right_stick(),
        );
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
