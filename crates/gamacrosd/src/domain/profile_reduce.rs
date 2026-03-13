use colored::Colorize;
use gamacros_workspace::ProfileEvent;

use crate::app::Gamacros;
use crate::domain::{DomainStep, RuntimeMode};
use crate::{print_error, print_info};

pub fn reduce_profile_event(
    profile_event: ProfileEvent,
    step: &mut DomainStep,
    gamacros: &mut Gamacros,
) {
    match profile_event {
        ProfileEvent::Changed(workspace) => {
            print_info!("profile changed, updating workspace");
            gamacros.set_workspace(workspace);
            step.set_shell = Some(gamacros.current_shell());
            step.wake_intents
                .push(crate::domain::WakeIntent::Reschedule);
            step.next_mode = Some(RuntimeMode::Active);
        }
        ProfileEvent::Removed => {
            gamacros.remove_workspace();
            step.set_shell = Some(gamacros.current_shell());
            step.wake_intents
                .push(crate::domain::WakeIntent::Reschedule);
            step.next_mode = Some(RuntimeMode::AwaitingProfile);
        }
        ProfileEvent::Error(error) => {
            print_error!("profile error: {error}");
        }
    }
}
