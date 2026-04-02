use std::{process::Command, time::Duration};

use colored::Colorize;
use gamacros_control::Performer;
use gamacros_gamepad::ControllerManager;

use gamacros_workspace::{MouseButton, MouseClickType};

use crate::{app::Effect, print_error, print_info};

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

    pub fn run_effect(&mut self, effect: Effect) {
        match effect {
            Effect::KeyTap(ref k) => {
                print_info!("ACTION: KeyTap combo={k:?}");
                match self.keypress.perform(k) {
                    Ok(()) => print_info!("  KeyTap OK"),
                    Err(e) => print_error!("  KeyTap FAILED: {e:?}"),
                }
            }
            Effect::KeyPress(ref k) => {
                print_info!("ACTION: KeyPress combo={k:?}");
                match self.keypress.press(k) {
                    Ok(()) => print_info!("  KeyPress OK"),
                    Err(e) => print_error!("  KeyPress FAILED: {e:?}"),
                }
            }
            Effect::KeyRelease(ref k) => {
                print_info!("ACTION: KeyRelease combo={k:?}");
                match self.keypress.release(k) {
                    Ok(()) => print_info!("  KeyRelease OK"),
                    Err(e) => print_error!("  KeyRelease FAILED: {e:?}"),
                }
            }
            Effect::Macros(ref m) => {
                print_info!("ACTION: Macros ({} combos)", m.len());
                for (i, k) in m.iter().enumerate() {
                    print_info!("  Macros[{i}] combo={k:?}");
                    match self.keypress.perform(k) {
                        Ok(()) => print_info!("  Macros[{i}] OK"),
                        Err(e) => print_error!("  Macros[{i}] FAILED: {e:?}"),
                    }
                }
            }
            Effect::Shell(ref s) => {
                print_info!("ACTION: Shell cmd={s}");
                let _ = self.run_shell(s);
            }
            Effect::MouseClick { button, click_type } => {
                print_info!(
                    "ACTION: MouseClick button={button:?} click_type={click_type:?}"
                );
                let enigo_button = match button {
                    MouseButton::Left => enigo::Button::Left,
                    MouseButton::Right => enigo::Button::Right,
                    MouseButton::Middle => enigo::Button::Middle,
                };
                let result = match click_type {
                    MouseClickType::Click => self.keypress.mouse_click(enigo_button),
                    MouseClickType::DoubleClick => {
                        self.keypress.mouse_double_click(enigo_button)
                    }
                };
                match result {
                    Ok(()) => print_info!("  MouseClick OK"),
                    Err(e) => print_error!("  MouseClick FAILED: {e:?}"),
                }
            }
            Effect::MousePress { button } => {
                print_info!("ACTION: MousePress button={button:?}");
                let enigo_button = match button {
                    MouseButton::Left => enigo::Button::Left,
                    MouseButton::Right => enigo::Button::Right,
                    MouseButton::Middle => enigo::Button::Middle,
                };
                match self.keypress.mouse_press(enigo_button) {
                    Ok(()) => print_info!("  MousePress OK"),
                    Err(e) => print_error!("  MousePress FAILED: {e:?}"),
                }
            }
            Effect::MouseRelease { button } => {
                print_info!("ACTION: MouseRelease button={button:?}");
                let enigo_button = match button {
                    MouseButton::Left => enigo::Button::Left,
                    MouseButton::Right => enigo::Button::Right,
                    MouseButton::Middle => enigo::Button::Middle,
                };
                match self.keypress.mouse_release(enigo_button) {
                    Ok(()) => print_info!("  MouseRelease OK"),
                    Err(e) => print_error!("  MouseRelease FAILED: {e:?}"),
                }
            }
            Effect::MouseMove { dx, dy } => {
                let _ = self.keypress.mouse_move(dx, dy);
            }
            Effect::Scroll { h, v } => {
                let started_at = std::time::Instant::now();
                print_info!("ACTION: Scroll h={h:.3} v={v:.3}");
                if h != 0.0 {
                    match self.keypress.scroll_x(h) {
                        Ok(()) => print_info!("  ScrollX OK value={h:.3}"),
                        Err(e) => {
                            print_error!("  ScrollX FAILED value={h:.3}: {e:?}")
                        }
                    }
                }
                if v != 0.0 {
                    match self.keypress.scroll_y(v) {
                        Ok(()) => print_info!("  ScrollY OK value={v:.3}"),
                        Err(e) => {
                            print_error!("  ScrollY FAILED value={v:.3}: {e:?}")
                        }
                    }
                }
                print_info!(
                    "  Scroll elapsed_us={} ",
                    started_at.elapsed().as_micros()
                );
            }
            Effect::Rumble { id, ms } => {
                print_info!("ACTION: Rumble id={id} ms={ms}");
                if let Some(h) = self.manager.controller(id) {
                    let _ = h.rumble(1.0, 1.0, Duration::from_millis(ms as u64));
                }
            }
            #[cfg(target_os = "macos")]
            Effect::RawModifierPress(key) => {
                let keycode = key.keycode();
                print_info!(
                    "ACTION: RawModifierPress key={key:?} keycode=0x{keycode:02x}"
                );
                match self.keypress.raw_modifier_press(keycode) {
                    Ok(()) => print_info!("  RawModifierPress OK"),
                    Err(e) => print_error!("  RawModifierPress FAILED: {e}"),
                }
            }
            #[cfg(target_os = "macos")]
            Effect::RawModifierRelease(key) => {
                let keycode = key.keycode();
                print_info!(
                    "ACTION: RawModifierRelease key={key:?} keycode=0x{keycode:02x}"
                );
                match self.keypress.raw_modifier_release(keycode) {
                    Ok(()) => print_info!("  RawModifierRelease OK"),
                    Err(e) => print_error!("  RawModifierRelease FAILED: {e}"),
                }
            }
            #[cfg(not(target_os = "macos"))]
            Effect::RawModifierPress(_) | Effect::RawModifierRelease(_) => {
                print_error!("ACTION: RawModifier not supported on this platform");
            }
        }
    }

    fn run_shell(&mut self, cmd: &str) -> Result<(), String> {
        let shell = self.shell.clone().unwrap_or(DEFAULT_SHELL.into());
        match Command::new(shell.into_string().as_str())
            .args(["-c", cmd])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {
                print_info!("shell command spawned");
                Ok(())
            }
            Err(e) => {
                print_error!("shell command error: {}", e);
                Err(e.to_string())
            }
        }
    }

    pub fn set_shell(&mut self, shell: Option<Box<str>>) {
        self.shell = shell;
    }
}
