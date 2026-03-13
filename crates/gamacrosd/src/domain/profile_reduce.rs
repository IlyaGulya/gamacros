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
            step.transition.shell = Some(crate::domain::ShellTransition::Set(
                gamacros.current_shell(),
            ));
            step.transition
                .wake_intents
                .push(crate::domain::WakeIntent::Reschedule);
            step.transition.mode =
                Some(crate::domain::ModeTransition::Set(RuntimeMode::Active));
        }
        ProfileEvent::Removed => {
            gamacros.remove_workspace();
            step.transition.shell = Some(crate::domain::ShellTransition::Set(
                gamacros.current_shell(),
            ));
            step.transition
                .wake_intents
                .push(crate::domain::WakeIntent::Reschedule);
            step.transition.mode = Some(crate::domain::ModeTransition::Set(
                RuntimeMode::AwaitingProfile,
            ));
        }
        ProfileEvent::Error(error) => {
            print_error!("profile error: {error}");
        }
    }
}
