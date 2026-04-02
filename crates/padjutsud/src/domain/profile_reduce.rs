use colored::Colorize;
use padjutsu_workspace::ProfileEvent;

use crate::app::Padjutsu;
use crate::domain::{DomainStep, RuntimeMode, WakeTransition};
use crate::{print_error, print_info};

pub fn reduce_profile_event(
    profile_event: ProfileEvent,
    step: &mut DomainStep,
    padjutsu: &mut Padjutsu,
) {
    match profile_event {
        ProfileEvent::Changed(workspace) => {
            print_info!("profile changed, updating workspace");
            padjutsu.set_workspace(workspace);
            step.transition.shell = Some(crate::domain::ShellTransition::Set(
                padjutsu.current_shell(),
            ));
            step.transition.wake.push(WakeTransition::Reschedule);
            step.transition.mode =
                Some(crate::domain::ModeTransition::Set(RuntimeMode::Active));
        }
        ProfileEvent::Removed => {
            padjutsu.remove_workspace();
            step.transition.shell = Some(crate::domain::ShellTransition::Set(
                padjutsu.current_shell(),
            ));
            step.transition.wake.push(WakeTransition::Reschedule);
            step.transition.mode = Some(crate::domain::ModeTransition::Set(
                RuntimeMode::AwaitingProfile,
            ));
        }
        ProfileEvent::Error(error) => {
            print_error!("profile error: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use ahash::{AHashMap, AHashSet};

    use super::*;
    use crate::domain::{ModeTransition, WakeTransition};
    use padjutsu_workspace::Profile;

    fn profile_with_shell(shell: &str) -> Profile {
        Profile {
            controllers: AHashMap::new(),
            blacklist: AHashSet::new(),
            rules: AHashMap::new(),
            shell: Some(shell.into()),
        }
    }

    #[test]
    fn changed_profile_transitions_runtime_to_active() {
        let mut step = DomainStep::continue_();
        let mut padjutsu = Padjutsu::new();

        reduce_profile_event(
            ProfileEvent::Changed(profile_with_shell("/bin/zsh")),
            &mut step,
            &mut padjutsu,
        );

        assert!(matches!(
            step.transition.mode,
            Some(ModeTransition::Set(RuntimeMode::Active))
        ));
        assert!(matches!(
            step.transition.shell,
            Some(crate::domain::ShellTransition::Set(Some(_)))
        ));
        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::Reschedule]
        ));
    }

    #[test]
    fn removed_profile_transitions_runtime_to_awaiting_profile() {
        let mut step = DomainStep::continue_();
        let mut padjutsu = Padjutsu::new();
        padjutsu.set_workspace(profile_with_shell("/bin/zsh"));

        reduce_profile_event(ProfileEvent::Removed, &mut step, &mut padjutsu);

        assert!(matches!(
            step.transition.mode,
            Some(ModeTransition::Set(RuntimeMode::AwaitingProfile))
        ));
        assert!(matches!(
            step.transition.wake.as_slice(),
            [WakeTransition::Reschedule]
        ));
    }
}
