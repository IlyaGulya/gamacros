use std::time::{Duration, Instant};

use colored::Colorize;

use crate::app::Gamacros;
use crate::domain::{DomainEvent, TimerEvent};
use crate::print_debug;

pub struct WakeState {
    pub need_reschedule: bool,
    pub ticking_enabled: bool,
    pub fast_mode: bool,
    pub fast_until: Instant,
    pub next_tick_due: Option<Instant>,
}

pub struct WakePlan {
    pub repeat_due: Option<Instant>,
    pub button_repeat_due: Option<Instant>,
    pub next_due: Option<Instant>,
}

impl WakeState {
    pub fn new(now: Instant) -> Self {
        Self {
            need_reschedule: true,
            ticking_enabled: false,
            fast_mode: false,
            fast_until: now,
            next_tick_due: None,
        }
    }
}

pub fn reschedule_wake(
    gamacros: &Gamacros,
    wake_state: &mut WakeState,
    idle_period: Duration,
    fast_period: Duration,
) -> WakePlan {
    let now = Instant::now();
    if gamacros.needs_tick() {
        let was_ticking_enabled = wake_state.ticking_enabled;
        let previous_tick_due = wake_state.next_tick_due;
        if !wake_state.ticking_enabled {
            wake_state.fast_mode = gamacros.wants_fast_tick();
            if wake_state.fast_mode {
                wake_state.fast_until = now + Duration::from_millis(250);
            }
        }
        let period = if wake_state.fast_mode {
            if gamacros.wants_continuous_tick_mode() {
                Duration::from_millis(gamacros.continuous_tick_ms().unwrap_or(4))
            } else {
                fast_period
            }
        } else {
            idle_period
        };
        let desired_tick_due = now + period;
        wake_state.next_tick_due = match previous_tick_due {
            Some(existing_due) if was_ticking_enabled && existing_due > now => {
                Some(core::cmp::min(existing_due, desired_tick_due))
            }
            _ => Some(desired_tick_due),
        };
        wake_state.ticking_enabled = true;
        let next_tick_in = wake_state
            .next_tick_due
            .map(|due| due.saturating_duration_since(now).as_millis())
            .unwrap_or_default();
        print_debug!(
            "wake reschedule: ticking_enabled=true fast_mode={} next_tick_in_ms={}",
            wake_state.fast_mode,
            next_tick_in
        );
    } else {
        wake_state.next_tick_due = None;
        wake_state.ticking_enabled = false;
        wake_state.fast_mode = false;
        print_debug!("wake reschedule: ticking disabled");
    }

    let repeat_due = gamacros.next_repeat_due();
    let button_repeat_due = gamacros.next_button_repeat_due();
    let mut next_due = wake_state.next_tick_due;
    for candidate in [repeat_due, button_repeat_due] {
        next_due = match (next_due, candidate) {
            (Some(a), Some(b)) => Some(core::cmp::min(a, b)),
            (Some(a), None) => Some(a),
            (None, b) => b,
        };
    }

    WakePlan {
        repeat_due,
        button_repeat_due,
        next_due,
    }
}

pub fn has_overdue_work(
    gamacros: &Gamacros,
    wake_state: &WakeState,
    now: Instant,
) -> bool {
    wake_state.next_tick_due.is_some_and(|due| due <= now)
        || gamacros.next_repeat_due().is_some_and(|due| due <= now)
        || gamacros
            .next_button_repeat_due()
            .is_some_and(|due| due <= now)
}

pub fn overdue_wake_event(
    gamacros: &Gamacros,
    wake_state: &WakeState,
    now: Instant,
) -> Option<DomainEvent> {
    let tick_due = wake_state.next_tick_due.is_some_and(|due| due <= now);
    let stick_repeat_due = gamacros.next_repeat_due().is_some_and(|due| due <= now);
    let button_repeat_due = gamacros
        .next_button_repeat_due()
        .is_some_and(|due| due <= now);

    if has_overdue_work(gamacros, wake_state, now) {
        print_debug!(
            "processing overdue wake: tick_due={} stick_repeat_due={} button_repeat_due={}",
            tick_due,
            stick_repeat_due,
            button_repeat_due
        );
        Some(DomainEvent::Timer(TimerEvent::Wake))
    } else {
        None
    }
}
