use std::{process::Command, time::Duration};

use colored::Colorize;
use gamacros_control::Performer;
use gamacros_gamepad::ControllerManager;

use gamacros_workspace::{MouseButton, MouseClickType};

use crate::{app::Action, print_error, print_info};

const DEFAULT_SHELL: &str = "/bin/zsh";

pub struct ActionRunner<'a> {
    keypress: &'a mut Performer,
    manager: &'a ControllerManager,
    shell: Option<Box<str>>,
}

impl<'a> ActionRunner<'a> {
    pub fn new(keypress: &'a mut Performer, manager: &'a ControllerManager) -> Self {
        Self {
            keypress,
            manager,
            shell: None,
        }
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::KeyTap(ref k) => {
                print_info!("ACTION: KeyTap combo={k:?}");
                match self.keypress.perform(k) {
                    Ok(()) => print_info!("  KeyTap OK"),
                    Err(e) => print_error!("  KeyTap FAILED: {e:?}"),
                }
            }
            Action::KeyPress(ref k) => {
                print_info!("ACTION: KeyPress combo={k:?}");
                match self.keypress.press(k) {
                    Ok(()) => print_info!("  KeyPress OK"),
                    Err(e) => print_error!("  KeyPress FAILED: {e:?}"),
                }
            }
            Action::KeyRelease(ref k) => {
                print_info!("ACTION: KeyRelease combo={k:?}");
                match self.keypress.release(k) {
                    Ok(()) => print_info!("  KeyRelease OK"),
                    Err(e) => print_error!("  KeyRelease FAILED: {e:?}"),
                }
            }
            Action::Macros(ref m) => {
                print_info!("ACTION: Macros ({} combos)", m.len());
                for (i, k) in m.iter().enumerate() {
                    print_info!("  Macros[{i}] combo={k:?}");
                    match self.keypress.perform(k) {
                        Ok(()) => print_info!("  Macros[{i}] OK"),
                        Err(e) => print_error!("  Macros[{i}] FAILED: {e:?}"),
                    }
                }
            }
            Action::Shell(ref s) => {
                print_info!("ACTION: Shell cmd={s}");
                let _ = self.run_shell(s);
            }
            Action::MouseClick { button, click_type } => {
                print_info!("ACTION: MouseClick button={button:?} click_type={click_type:?}");
                let enigo_button = match button {
                    MouseButton::Left => enigo::Button::Left,
                    MouseButton::Right => enigo::Button::Right,
                    MouseButton::Middle => enigo::Button::Middle,
                };
                let result = match click_type {
                    MouseClickType::Click => self.keypress.mouse_click(enigo_button),
                    MouseClickType::DoubleClick => self.keypress.mouse_double_click(enigo_button),
                };
                match result {
                    Ok(()) => print_info!("  MouseClick OK"),
                    Err(e) => print_error!("  MouseClick FAILED: {e:?}"),
                }
            }
            Action::MouseMove { dx, dy } => {
                let _ = self.keypress.mouse_move(dx, dy);
            }
            Action::Scroll { h, v } => {
                if h != 0 {
                    let _ = self.keypress.scroll_x(h);
                }
                if v != 0 {
                    let _ = self.keypress.scroll_y(v);
                }
            }
            Action::Rumble { id, ms } => {
                print_info!("ACTION: Rumble id={id} ms={ms}");
                if let Some(h) = self.manager.controller(id) {
                    let _ = h.rumble(1.0, 1.0, Duration::from_millis(ms as u64));
                }
            }
            #[cfg(target_os = "macos")]
            Action::RawModifierPress(key) => {
                let keycode = key.keycode();
                print_info!("ACTION: RawModifierPress key={key:?} keycode=0x{keycode:02x}");
                match self.keypress.raw_modifier_press(keycode) {
                    Ok(()) => print_info!("  RawModifierPress OK"),
                    Err(e) => print_error!("  RawModifierPress FAILED: {e}"),
                }
            }
            #[cfg(target_os = "macos")]
            Action::RawModifierRelease(key) => {
                let keycode = key.keycode();
                print_info!("ACTION: RawModifierRelease key={key:?} keycode=0x{keycode:02x}");
                match self.keypress.raw_modifier_release(keycode) {
                    Ok(()) => print_info!("  RawModifierRelease OK"),
                    Err(e) => print_error!("  RawModifierRelease FAILED: {e}"),
                }
            }
            #[cfg(not(target_os = "macos"))]
            Action::RawModifierPress(_) | Action::RawModifierRelease(_) => {
                print_error!("ACTION: RawModifier not supported on this platform");
            }
        }
    }

    fn run_shell(&mut self, cmd: &str) -> Result<String, String> {
        let shell = self.shell.clone().unwrap_or(DEFAULT_SHELL.into());
        let result = Command::new(shell.into_string().as_str())
            .args(["-c", cmd])
            .output();

        match result {
            Ok(output) => {
                print_info!(
                    "shell command output: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }
            Err(e) => {
                print_error!("shell command error: {}", e);
                Err(e.to_string())
            }
        }
    }

    pub fn set_shell(&mut self, shell: Box<str>) {
        self.shell = Some(shell);
    }
}
