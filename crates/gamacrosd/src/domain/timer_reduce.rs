use std::time::Duration;

use colored::Colorize;

use crate::app::Gamacros;
use crate::domain::{DomainStep, RuntimeState, TimerEvent, WakeIntent, WakeState};
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
                    step.wake_intents.push(WakeIntent::DisableFastMode);
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
    }
}
