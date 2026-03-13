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
    step.transition.shell = Some(crate::domain::ShellTransition::Set(
        gamacros.current_shell(),
    ));
    step.transition.wake_intents.push(WakeIntent::Reschedule);
}
