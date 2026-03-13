use crate::activity::ActivityEvent;
use crate::app::Gamacros;
use crate::domain::{DomainStep, WakeIntent};

pub fn reduce_activity_event(
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
