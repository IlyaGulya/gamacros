use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;
use ahash::AHashMap;

use colored::Colorize;

use gamacros_control::KeyCombo;
use gamacros_bit_mask::Bitmask;
use gamacros_gamepad::{Button, ControllerId, ControllerInfo, Axis as CtrlAxis};
use gamacros_workspace::{
    ButtonAction, ControllerSettings, Macros, MouseButton, MouseClickType,
    Profile, RawModifierKey, StickRules, StickMode,
};

use crate::{app::ButtonPhase, print_debug, print_info};
use super::stick::{StickProcessor, CompiledStickRules};
use super::stick::util::axis_index as stick_axis_index;

#[derive(Debug, Clone)]
pub enum Action {
    KeyPress(KeyCombo),
    KeyRelease(KeyCombo),
    KeyTap(KeyCombo),
    Macros(Arc<Macros>),
    Shell(String),
    MouseClick { button: MouseButton, click_type: MouseClickType },
    MouseMove { dx: i32, dy: i32 },
    Scroll { h: i32, v: i32 },
    Rumble { id: ControllerId, ms: u32 },
    RawModifierPress(RawModifierKey),
    RawModifierRelease(RawModifierKey),
}

#[derive(Debug)]
struct ControllerState {
    mapping: ControllerSettings,
    pressed: Bitmask<Button>,
    rumble: bool,
    axes: [f32; 6],
}

const DEFAULT_REPEAT_DELAY_MS: u64 = 400;
const DEFAULT_REPEAT_INTERVAL_MS: u64 = 50;

struct ButtonRepeatTask {
    key: KeyCombo,
    interval_ms: u64,
    next_fire: Instant,
    delay_done: bool,
}

pub struct Gamacros {
    pub workspace: Option<Profile>,
    active_app: Box<str>,
    controllers: AHashMap<ControllerId, ControllerState>,
    sticks: RefCell<StickProcessor>,
    active_stick_rules: Option<Arc<StickRules>>, // keep original for potential future use
    compiled_stick_rules: Option<CompiledStickRules>,
    axes_scratch: Vec<(ControllerId, [f32; 6])>,
    button_repeats: AHashMap<(ControllerId, Button), ButtonRepeatTask>,
}

impl Default for Gamacros {
    fn default() -> Self {
        Self::new()
    }
}

impl Gamacros {
    pub fn new() -> Self {
        Self {
            workspace: None,
            active_app: "".into(),
            controllers: AHashMap::new(),
            sticks: RefCell::new(StickProcessor::new()),
            active_stick_rules: None,
            compiled_stick_rules: None,
            axes_scratch: Vec::new(),
            button_repeats: AHashMap::new(),
        }
    }

    pub fn is_known(&self, id: ControllerId) -> bool {
        self.controllers.contains_key(&id)
    }

    pub fn remove_workspace(&mut self) {
        self.workspace = None;
        self.active_stick_rules = None;
        self.compiled_stick_rules = None;
    }

    pub fn set_workspace(&mut self, workspace: Profile) {
        self.workspace = Some(workspace);
        // Recompute stick rules for current active app (workspace may have changed)
        if !self.active_app.is_empty() {
            if let Some(ws) = self.workspace.as_ref() {
                if let Some(app_rules) = ws.rules.get(&*self.active_app).or_else(|| ws.rules.get("common")) {
                    self.active_stick_rules =
                        Some(Arc::new(app_rules.sticks.clone()));
                    self.compiled_stick_rules = self
                        .active_stick_rules
                        .as_deref()
                        .map(CompiledStickRules::from_rules);
                } else {
                    self.active_stick_rules = None;
                    self.compiled_stick_rules = None;
                }
            }
        }
    }

    pub fn add_controller(&mut self, info: ControllerInfo) {
        print_info!(
            "add controller - {0} id={1} vid=0x{2:x} pid=0x{3:x}",
            info.name,
            info.id,
            info.vendor_id,
            info.product_id
        );

        let settings = self.workspace.as_ref()
            .and_then(|ws| ws.controllers.get(&(info.vendor_id, info.product_id)).cloned())
            .unwrap_or_default();
        let state = ControllerState {
            mapping: settings,
            pressed: Bitmask::empty(),
            rumble: info.supports_rumble,
            axes: [0.0; 6],
        };
        if self.is_known(info.id) {
            print_debug!("controller already known - id={0}", info.id);
        }
        self.controllers.insert(info.id, state);
    }

    pub fn remove_controller(&mut self, id: ControllerId) {
        print_info!("remove device - {id:x}");
        self.controllers.remove(&id);
    }

    pub fn supports_rumble(&self, id: ControllerId) -> bool {
        self.controllers.get(&id).map(|s| s.rumble).unwrap_or(false)
    }

    pub fn set_active_app(&mut self, app: &str) {
        if self.active_app.as_ref() == app {
            return;
        }
        if self.active_app.as_ref() == "" {
            print_debug!("got active app - {app}");
        } else {
            print_debug!("app change - {app}");
        }

        self.active_app = app.into();
        self.sticks.borrow_mut().on_app_change();
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };

        self.active_stick_rules = workspace
            .rules
            .get(&*self.active_app)
            .or_else(|| workspace.rules.get("common"))
            .map(|r| Arc::new(r.sticks.clone()));

        self.compiled_stick_rules = self
            .active_stick_rules
            .as_deref()
            .map(CompiledStickRules::from_rules);
    }

    pub fn get_active_app(&self) -> &str {
        &self.active_app
    }

    pub fn get_compiled_stick_rules(&self) -> Option<&CompiledStickRules> {
        self.compiled_stick_rules.as_ref()
    }

    pub fn on_axis_motion(&mut self, id: ControllerId, axis: CtrlAxis, value: f32) {
        let idx = stick_axis_index(axis);
        if let Some(st) = self.controllers.get_mut(&id) {
            st.axes[idx] = value;
        }
    }

    pub fn on_controller_disconnected(&mut self, id: ControllerId) {
        self.sticks.borrow_mut().release_all_for(id);
    }

    pub fn on_tick_with<F: FnMut(Action)>(&mut self, sink: F) {
        let bindings_owned = self.get_compiled_stick_rules().cloned();
        self.axes_scratch.clear();
        self.axes_scratch.reserve(self.controllers.len());
        for (id, st) in self.controllers.iter() {
            self.axes_scratch.push((*id, st.axes));
        }
        self.sticks.borrow_mut().on_tick_with(
            bindings_owned.as_ref(),
            &self.axes_scratch,
            sink,
        );
    }

    /// Return next due time for any repeat task, if any.
    pub fn next_repeat_due(&self) -> Option<std::time::Instant> {
        // Borrow mutably internally to read/update heap staleness cheaply.
        // Safety: RefCell ensures single mutable borrow.
        self.sticks.borrow_mut().next_repeat_due()
    }

    /// Process repeat tasks due up to `now`.
    pub fn process_due_repeats<F: FnMut(Action)>(
        &self,
        now: std::time::Instant,
        mut sink: F,
    ) {
        self.sticks.borrow_mut().process_due_repeats(now, &mut sink);
    }

    /// Return next due time for any button repeat task, if any.
    pub fn next_button_repeat_due(&self) -> Option<Instant> {
        self.button_repeats.values().map(|t| t.next_fire).min()
    }

    /// Process button repeat tasks due up to `now`.
    pub fn process_button_repeats<F: FnMut(Action)>(&mut self, now: Instant, sink: &mut F) {
        for task in self.button_repeats.values_mut() {
            if now >= task.next_fire {
                sink(Action::KeyTap(task.key.clone()));
                if !task.delay_done {
                    task.delay_done = true;
                }
                task.next_fire = now + std::time::Duration::from_millis(task.interval_ms);
            }
        }
    }

    /// Whether any button repeat tasks are active.
    pub fn has_active_button_repeats(&self) -> bool {
        !self.button_repeats.is_empty()
    }

    /// Whether any periodic processing is needed right now.
    /// True when there are tick-requiring stick modes and some axis deviates from neutral,
    /// or when repeat tasks are active (to drain their timers).
    pub fn needs_tick(&self) -> bool {
        (self.has_tick_modes() && self.has_axis_activity(0.05))
            || self.sticks.borrow().has_active_repeats()
            || self.has_active_button_repeats()
    }

    /// Hint whether a faster tick would improve responsiveness.
    /// True when there is recent/ongoing axis activity or repeat tasks are active.
    pub fn wants_fast_tick(&self) -> bool {
        self.has_axis_activity(0.05)
            || self.sticks.borrow().has_active_repeats()
            || self.has_active_button_repeats()
    }

    /// Whether the current profile has any stick modes that require periodic ticks.
    fn has_tick_modes(&self) -> bool {
        let Some(bindings) = self.get_compiled_stick_rules() else {
            return false;
        };
        matches!(
            bindings.left(),
            Some(
                StickMode::Arrows(_)
                    | StickMode::Volume(_)
                    | StickMode::Brightness(_)
                    | StickMode::MouseMove(_)
                    | StickMode::Scroll(_)
            )
        ) || matches!(
            bindings.right(),
            Some(
                StickMode::Arrows(_)
                    | StickMode::Volume(_)
                    | StickMode::Brightness(_)
                    | StickMode::MouseMove(_)
                    | StickMode::Scroll(_)
            )
        )
    }

    /// Detect if any controller axis deviates beyond a small threshold.
    fn has_axis_activity(&self, threshold: f32) -> bool {
        if self.controllers.is_empty() {
            return false;
        }
        for (_id, st) in self.controllers.iter() {
            for v in st.axes.iter() {
                if v.abs() >= threshold {
                    return true;
                }
            }
        }
        false
    }

    pub fn on_button_with<F: FnMut(Action)>(
        &mut self,
        id: ControllerId,
        button: Button,
        phase: ButtonPhase,
        mut sink: F,
    ) {
        print_debug!("handle button - {id} {button:?} {phase:?}");
        let active_app = self.get_active_app();
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        let app_rules = match workspace.rules.get(active_app) {
            Some(r) => {
                print_debug!("using app-specific rules for {active_app}");
                r
            }
            None => match workspace.rules.get("common") {
                Some(r) => {
                    print_debug!("using common rules (no rules for {active_app})");
                    r
                }
                None => {
                    print_debug!("no rules found for {active_app} and no common rules");
                    return;
                }
            },
        };
        let Some(state) = self.controllers.get_mut(&id) else {
            print_debug!("ignoring button for unknown controller {id}");
            return;
        };
        let button = *state.mapping.mapping.get(&button).unwrap_or(&button);
        let rumble = state.rumble;

        // snapshot before change
        let prev_pressed = state.pressed;

        if phase == ButtonPhase::Pressed {
            state.pressed.insert(button);
        } else {
            state.pressed.remove(button);
        }

        // snapshot after change — drop mutable borrow of controllers after this
        let now_pressed = state.pressed;

        // First pass: find max_bits among rules that should fire
        let mut max_bits: u32 = 0;
        for (target, _rule) in app_rules.buttons.iter() {
            let was = prev_pressed.is_superset(target);
            let is_now = now_pressed.is_superset(target);
            let fire = match phase {
                ButtonPhase::Pressed => was != is_now,
                ButtonPhase::Released => was && !is_now,
            };
            if fire {
                let bits: u32 = target.count();
                if bits > max_bits {
                    max_bits = bits;
                }
            }
        }
        if max_bits == 0 {
            print_debug!("no matching rule for pressed={now_pressed:?} (button_rules={})", app_rules.buttons.len());
            return;
        }
        print_debug!("firing rule with max_bits={max_bits}");

        // Second pass: execute only rules with that cardinality
        for (target, rule) in app_rules.buttons.iter() {
            let was = prev_pressed.is_superset(target);
            let is_now = now_pressed.is_superset(target);
            let fire = match phase {
                ButtonPhase::Pressed => was != is_now,
                ButtonPhase::Released => was && !is_now,
            };
            if !fire || target.count() != max_bits {
                continue;
            }
            match phase {
                ButtonPhase::Pressed => {
                    if let Some(ms) = rule.vibrate {
                        if rumble {
                            sink(Action::Rumble { id, ms: ms as u32 });
                        }
                    }
                    match rule.action.clone() {
                        ButtonAction::Keystroke(k) => {
                            sink(Action::KeyTap((*k).clone()));
                            let delay_ms = rule.repeat_delay_ms.unwrap_or(DEFAULT_REPEAT_DELAY_MS);
                            let interval_ms = rule.repeat_interval_ms.unwrap_or(DEFAULT_REPEAT_INTERVAL_MS);
                            self.button_repeats.insert(
                                (id, button),
                                ButtonRepeatTask {
                                    key: (*k).clone(),
                                    interval_ms,
                                    next_fire: Instant::now() + std::time::Duration::from_millis(delay_ms),
                                    delay_done: false,
                                },
                            );
                        }
                        ButtonAction::TapKeystroke(k) => {
                            sink(Action::KeyTap((*k).clone()));
                        }
                        ButtonAction::Macros(m) => {
                            sink(Action::Macros(m));
                        }
                        ButtonAction::Shell(s) => {
                            print_debug!("shell command: {}", s);
                            sink(Action::Shell(s));
                        }
                        ButtonAction::MouseClick { button, click_type } => {
                            sink(Action::MouseClick { button, click_type });
                        }
                        ButtonAction::RawModifier(key) => {
                            sink(Action::RawModifierPress(key));
                        }
                    }
                }
                ButtonPhase::Released => {
                    match rule.action.clone() {
                        ButtonAction::Keystroke(_) => {
                            self.button_repeats.remove(&(id, button));
                        }
                        ButtonAction::RawModifier(key) => {
                            sink(Action::RawModifierRelease(key));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
