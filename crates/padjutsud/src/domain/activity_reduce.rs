use crate::activity::ActivityEvent;
use crate::app::Padjutsu;
use crate::domain::{DomainStep, WakeTransition};

pub fn reduce_activity_event(
    activity_event: ActivityEvent,
    step: &mut DomainStep,
    padjutsu: &mut Padjutsu,
) {
    let ActivityEvent::DidActivateApplication(bundle_id) = activity_event else {
        return;
    };
    padjutsu.set_active_app(&bundle_id);
    step.transition.shell = Some(crate::domain::ShellTransition::Set(
        padjutsu.current_shell(),
    ));
    step.transition.wake.push(WakeTransition::Reschedule);
}
