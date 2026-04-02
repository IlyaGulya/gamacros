use padjutsu_gamepad::ControllerManager;

use crate::api::Command as ApiCommand;
use crate::app::Effect;
use crate::domain::DomainStep;

pub fn reduce_api_command(
    command: ApiCommand,
    step: &mut DomainStep,
    manager: &ControllerManager,
) {
    match command {
        ApiCommand::Rumble { id, ms } => match id {
            Some(controller_id) => {
                step.transition.effects.push(Effect::Rumble {
                    id: controller_id,
                    ms,
                });
            }
            None => {
                for info in manager.controllers() {
                    step.transition
                        .effects
                        .push(Effect::Rumble { id: info.id, ms });
                }
            }
        },
    }
}
