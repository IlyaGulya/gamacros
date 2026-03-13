use std::time::Duration;

use colored::Colorize;

use crate::app::Gamacros;
use crate::domain::{DomainStep, RuntimeState, TimerEvent, WakeState, WakeTransition};
use crate::print_debug;

pub fn reduce_timer_event(
    timer_event: TimerEvent,
    step: &mut DomainStep,
    gamacros: &mut Gamacros,
    runtime_state: &RuntimeState,
    wake_state: &WakeState,
) {
    match timer_event {
        TimerEvent::Wake => {
            if !runtime_state.handles_timer_wake() {
                if wake_state.fast_mode {
                    step.transition.wake.push(WakeTransition::DisableFastMode);
                }
                print_debug!(
                    "ignoring timer wake while runtime mode is {:?}",
                    runtime_state.mode()
                );
                return;
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
                    step.transition.effects.extend(gamacros.on_tick_effects());
                    if gamacros.wants_fast_tick() {
                        let fast_until = now + Duration::from_millis(250);
                        step.transition
                            .wake
                            .push(WakeTransition::EnableFastModeUntil(fast_until));
                        print_debug!(
                            "wake timer: enabling fast mode until {fast_until:?}"
                        );
                    } else if wake_state.fast_mode && now >= wake_state.fast_until {
                        step.transition.wake.push(WakeTransition::DisableFastMode);
                        print_debug!("wake timer: disabling fast mode");
                    }
                }
            }
            let repeats_started_at = std::time::Instant::now();
            step.transition
                .effects
                .extend(gamacros.due_repeat_effects(now));
            print_debug!(
                "wake timer: stick repeats elapsed_us={}",
                repeats_started_at.elapsed().as_micros()
            );
            let button_repeats_started_at = std::time::Instant::now();
            step.transition
                .effects
                .extend(gamacros.button_repeat_effects(now));
            print_debug!(
                "wake timer: button repeats elapsed_us={}",
                button_repeats_started_at.elapsed().as_micros()
            );
            step.transition.wake.push(WakeTransition::Reschedule);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RuntimeMode, RuntimeState};

    #[test]
    fn wake_timer_disables_fast_mode_outside_active_runtime() {
        let mut step = DomainStep::continue_();
        let mut gamacros = Gamacros::new();
        let runtime_state = RuntimeState::new(RuntimeMode::AwaitingProfile);
        let mut wake_state = WakeState::new(std::time::Instant::now());
        wake_state.fast_mode = true;

        reduce_timer_event(
            TimerEvent::Wake,
            &mut step,
            &mut gamacros,
            &runtime_state,
            &wake_state,
        );

        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::DisableFastMode]
        ));
        assert!(step.transition.effects.is_empty());
    }
}
