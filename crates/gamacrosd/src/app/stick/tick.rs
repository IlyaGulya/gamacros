use colored::Colorize;
use gamacros_gamepad::ControllerId;
use gamacros_workspace::{Axis as ProfileAxis, StickMode, StickSide};

use crate::app::Effect;
use crate::print_debug;

use super::compiled::CompiledStickRules;
use super::repeat::{Direction, RepeatKind, RepeatTaskId, RepeatReg, StickProcessor};
use super::StepperMode;
use super::util::{
    axis_index, axes_for_side, invert_xy, magnitude2d, normalize_after_deadzone,
};

#[inline]
fn trigger_scroll_boost(
    axes: [f32; 6],
    params: &gamacros_workspace::ScrollParams,
) -> (f32, f32) {
    let lt = axes[axis_index(gamacros_gamepad::Axis::LeftTrigger)].max(0.0);
    let rt = axes[axis_index(gamacros_gamepad::Axis::RightTrigger)].max(0.0);
    let trigger = lt.max(rt).clamp(0.0, 1.0);
    let boost = 1.0
        + params.runtime.trigger_boost_max
            * trigger.powf(params.runtime.trigger_boost_gamma);
    (trigger, boost)
}

impl StickProcessor {
    pub fn on_tick_with<F: FnMut(Effect)>(
        &mut self,
        bindings: Option<&CompiledStickRules>,
        axes_list: &[(ControllerId, [f32; 6])],
        mut sink: F,
    ) {
        if axes_list.is_empty() && !self.has_active_repeats() {
            return;
        }
        let Some(bindings) = bindings else {
            return;
        };

        let now = std::time::Instant::now();
        let started_at = now;
        let dt_s = self.tick_dt_s(now);
        self.generation = self.generation.wrapping_add(1);
        print_debug!(
            "stick processor tick: generation={} controllers={} has_repeats={} dt_s={dt_s:.4} left_mode={:?} right_mode={:?}",
            self.generation,
            axes_list.len(),
            self.has_active_repeats(),
            bindings.left(),
            bindings.right()
        );

        if matches!(bindings.left(), Some(StickMode::Arrows(_)))
            || matches!(bindings.right(), Some(StickMode::Arrows(_)))
        {
            self.tick_arrows(now, &mut sink, axes_list, bindings);
        }
        if matches!(bindings.left(), Some(StickMode::Volume(_)))
            || matches!(bindings.right(), Some(StickMode::Volume(_)))
        {
            self.tick_stepper(
                now,
                &mut sink,
                axes_list,
                bindings,
                StepperMode::Volume,
            );
        }
        if matches!(bindings.left(), Some(StickMode::Brightness(_)))
            || matches!(bindings.right(), Some(StickMode::Brightness(_)))
        {
            self.tick_stepper(
                now,
                &mut sink,
                axes_list,
                bindings,
                StepperMode::Brightness,
            );
        }
        if matches!(bindings.left(), Some(StickMode::MouseMove(_)))
            || matches!(bindings.right(), Some(StickMode::MouseMove(_)))
        {
            self.tick_mouse(dt_s, &mut sink, axes_list, bindings);
        }
        if matches!(bindings.left(), Some(StickMode::Scroll(_)))
            || matches!(bindings.right(), Some(StickMode::Scroll(_)))
        {
            self.tick_scroll(dt_s, &mut sink, axes_list, bindings);
        }

        // Repeat draining is now event-driven, cleanup still needs to run per generation
        self.repeater_cleanup_inactive();
        print_debug!(
            "stick processor tick done: generation={} elapsed_us={}",
            self.generation,
            started_at.elapsed().as_micros()
        );
    }

    fn tick_dt_s(&mut self, now: std::time::Instant) -> f32 {
        const DEFAULT_DT_S: f32 = 0.010;
        const MIN_DT_S: f32 = 0.005;
        const MAX_DT_S: f32 = 0.050;

        let dt_s = self
            .last_tick_at
            .map(|last_tick_at| {
                now.saturating_duration_since(last_tick_at).as_secs_f32()
            })
            .unwrap_or(DEFAULT_DT_S)
            .clamp(MIN_DT_S, MAX_DT_S);
        self.last_tick_at = Some(now);
        dt_s
    }

    pub fn has_active_repeats(&self) -> bool {
        for (_cid, ctrl) in self.controllers.iter() {
            for side in ctrl.sides.iter() {
                if side.arrows.iter().any(|s| s.is_some())
                    || side.volume.iter().any(|s| s.is_some())
                    || side.brightness.iter().any(|s| s.is_some())
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn has_active_repeats_for(&self, id: ControllerId) -> bool {
        let Some(ctrl) = self.controllers.get(&id) else {
            return false;
        };

        ctrl.sides.iter().any(|side| {
            side.arrows.iter().any(|s| s.is_some())
                || side.volume.iter().any(|s| s.is_some())
                || side.brightness.iter().any(|s| s.is_some())
        })
    }

    pub fn has_active_repeats_for_side(
        &self,
        id: ControllerId,
        side: StickSide,
    ) -> bool {
        let Some(ctrl) = self.controllers.get(&id) else {
            return false;
        };
        let side = &ctrl.sides[super::util::side_index(&side)];

        side.arrows.iter().any(|s| s.is_some())
            || side.volume.iter().any(|s| s.is_some())
            || side.brightness.iter().any(|s| s.is_some())
    }

    fn tick_arrows(
        &mut self,
        now: std::time::Instant,
        sink: &mut impl FnMut(Effect),
        axes_list: &[(ControllerId, [f32; 6])],
        bindings: &CompiledStickRules,
    ) {
        let mut regs = std::mem::take(&mut self.regs);
        regs.clear();
        for (id, axes) in axes_list.iter().cloned() {
            if let Some(StickMode::Arrows(params)) = bindings.left() {
                let (x0, y0) = axes_for_side(axes, &StickSide::Left);
                let (x, y) = invert_xy(x0, y0, params.invert_x, !params.invert_y);
                let mag2 = x * x + y * y;
                let dead2 = params.deadzone * params.deadzone;
                let new_dir = if mag2 < dead2 {
                    None
                } else {
                    Self::quantize_direction(x, y)
                };
                print_debug!(
                    "stick arrows: controller={id} side=Left raw=({x0:.3},{y0:.3}) adjusted=({x:.3},{y:.3}) mag2={mag2:.3} dead2={dead2:.3} dir={new_dir:?}"
                );
                if let Some(dir) = new_dir {
                    let task_id = RepeatTaskId {
                        controller: id,
                        side: StickSide::Left,
                        kind: RepeatKind::Arrow(dir),
                    };
                    let key = Self::get_direction_key(dir);
                    regs.push(RepeatReg {
                        id: task_id,
                        key,
                        fire_on_activate: true,
                        initial_delay_ms: params.repeat_delay_ms,
                        interval_ms: params.repeat_interval_ms,
                    });
                }
            }
            if let Some(StickMode::Arrows(params)) = bindings.right() {
                let (x0, y0) = axes_for_side(axes, &StickSide::Right);
                let (x, y) = invert_xy(x0, y0, params.invert_x, !params.invert_y);
                let mag2 = x * x + y * y;
                let dead2 = params.deadzone * params.deadzone;
                let new_dir = if mag2 < dead2 {
                    None
                } else {
                    Self::quantize_direction(x, y)
                };
                print_debug!(
                    "stick arrows: controller={id} side=Right raw=({x0:.3},{y0:.3}) adjusted=({x:.3},{y:.3}) mag2={mag2:.3} dead2={dead2:.3} dir={new_dir:?}"
                );
                if let Some(dir) = new_dir {
                    let task_id = RepeatTaskId {
                        controller: id,
                        side: StickSide::Right,
                        kind: RepeatKind::Arrow(dir),
                    };
                    let key = Self::get_direction_key(dir);
                    regs.push(RepeatReg {
                        id: task_id,
                        key,
                        fire_on_activate: true,
                        initial_delay_ms: params.repeat_delay_ms,
                        interval_ms: params.repeat_interval_ms,
                    });
                }
            }
        }
        for reg in regs.drain(..) {
            if let Some(a) = self.repeater_register(reg, now) {
                (sink)(a);
            }
        }
        self.regs = regs;
    }

    fn tick_stepper(
        &mut self,
        now: std::time::Instant,
        sink: &mut impl FnMut(Effect),
        axes_list: &[(ControllerId, [f32; 6])],
        bindings: &CompiledStickRules,
        mode: StepperMode,
    ) {
        let mut regs = std::mem::take(&mut self.regs);
        regs.clear();
        for (cid, axes) in axes_list.iter().cloned() {
            if let Some(step_params) = match (&mode, bindings.left()) {
                (StepperMode::Volume, Some(StickMode::Volume(p))) => Some(p),
                (StepperMode::Brightness, Some(StickMode::Brightness(p))) => Some(p),
                _ => None,
            } {
                let (vx, vy) = (
                    axes[axis_index(gamacros_gamepad::Axis::LeftX)],
                    axes[axis_index(gamacros_gamepad::Axis::LeftY)],
                );
                let v = match step_params.axis {
                    ProfileAxis::X => vx,
                    ProfileAxis::Y => vy,
                };
                let mag = v.abs();
                if mag >= step_params.deadzone {
                    let t = mag;
                    let interval_ms = (step_params.max_interval_ms as f32)
                        + (1.0 - t)
                            * ((step_params.min_interval_ms as f32)
                                - (step_params.max_interval_ms as f32));
                    let positive = v >= 0.0;
                    let key = mode.key_for(positive);
                    let kind = mode.kind_for(step_params.axis, positive);
                    let task_id = RepeatTaskId {
                        controller: cid,
                        side: StickSide::Left,
                        kind,
                    };
                    regs.push(RepeatReg {
                        id: task_id,
                        key,
                        fire_on_activate: true,
                        initial_delay_ms: 0,
                        interval_ms: interval_ms as u64,
                    });
                    print_debug!(
                        "stick stepper: controller={cid} side=Left mode={mode:?} axis={:?} value={v:.3} mag={mag:.3} interval_ms={} positive={positive}",
                        step_params.axis,
                        interval_ms as u64
                    );
                }
            }
            if let Some(step_params) = match (&mode, bindings.right()) {
                (StepperMode::Volume, Some(StickMode::Volume(p))) => Some(p),
                (StepperMode::Brightness, Some(StickMode::Brightness(p))) => Some(p),
                _ => None,
            } {
                let (vx, vy) = (
                    axes[axis_index(gamacros_gamepad::Axis::RightX)],
                    axes[axis_index(gamacros_gamepad::Axis::RightY)],
                );
                let v = match step_params.axis {
                    ProfileAxis::X => vx,
                    ProfileAxis::Y => vy,
                };
                let mag = v.abs();
                if mag >= step_params.deadzone {
                    let t = mag;
                    let interval_ms = (step_params.max_interval_ms as f32)
                        + (1.0 - t)
                            * ((step_params.min_interval_ms as f32)
                                - (step_params.max_interval_ms as f32));
                    let positive = v >= 0.0;
                    let key = mode.key_for(positive);
                    let kind = mode.kind_for(step_params.axis, positive);
                    let task_id = RepeatTaskId {
                        controller: cid,
                        side: StickSide::Right,
                        kind,
                    };
                    regs.push(RepeatReg {
                        id: task_id,
                        key,
                        fire_on_activate: true,
                        initial_delay_ms: 0,
                        interval_ms: interval_ms as u64,
                    });
                    print_debug!(
                        "stick stepper: controller={cid} side=Right mode={mode:?} axis={:?} value={v:.3} mag={mag:.3} interval_ms={} positive={positive}",
                        step_params.axis,
                        interval_ms as u64
                    );
                }
            }
        }
        for reg in regs.drain(..) {
            if let Some(a) = self.repeater_register(reg, now) {
                (sink)(a);
            }
        }
        self.regs = regs;
    }

    fn tick_mouse(
        &mut self,
        dt_s: f32,
        sink: &mut impl FnMut(Effect),
        axes_list: &[(ControllerId, [f32; 6])],
        bindings: &CompiledStickRules,
    ) {
        for (_cid, axes) in axes_list.iter().cloned() {
            if let Some(StickMode::MouseMove(params)) = bindings.left() {
                let alpha = Self::mouse_smoothing_alpha(
                    dt_s,
                    params.runtime.smoothing_window_ms,
                );
                let (x0, y0) = axes_for_side(axes, &StickSide::Left);
                let sidx = super::util::side_index(&StickSide::Left);
                let side =
                    &mut self.controllers.entry(_cid).or_default().sides[sidx];
                let (raw_x, raw_y) =
                    invert_xy(x0, y0, params.invert_x, params.invert_y);
                side.mouse_filtered.0 += alpha * (raw_x - side.mouse_filtered.0);
                side.mouse_filtered.1 += alpha * (raw_y - side.mouse_filtered.1);
                let (x, y) = side.mouse_filtered;
                let mag_raw = magnitude2d(x, y);
                if mag_raw >= params.deadzone {
                    let base = normalize_after_deadzone(mag_raw, params.deadzone);
                    let mag = Self::fast_gamma(base, params.gamma);
                    if mag > 0.0 {
                        let dir_x = x / mag_raw;
                        let dir_y = y / mag_raw;
                        let speed_px_s = params.max_speed_px_s * mag;
                        let accum = &mut side.mouse_accum;
                        accum.0 += speed_px_s * dir_x * dt_s;
                        accum.1 += speed_px_s * dir_y * dt_s;
                        let dx = accum.0.trunc() as i32;
                        let dy = accum.1.trunc() as i32;
                        if dx != 0 || dy != 0 {
                            print_debug!(
                                "stick mouse: controller={_cid} side=Left raw=({x0:.3},{y0:.3}) filtered=({x:.3},{y:.3}) mag={mag:.3} speed_px_s={speed_px_s:.1} move=({dx},{dy}) accum=({:.3},{:.3})"
                                ,accum.0,accum.1
                            );
                            (sink)(Effect::MouseMove { dx, dy });
                            accum.0 -= dx as f32;
                            accum.1 -= dy as f32;
                        }
                    }
                } else {
                    side.mouse_filtered = (0.0, 0.0);
                    side.mouse_accum = (0.0, 0.0);
                }
            }
            if let Some(StickMode::MouseMove(params)) = bindings.right() {
                let alpha = Self::mouse_smoothing_alpha(
                    dt_s,
                    params.runtime.smoothing_window_ms,
                );
                let (x0, y0) = axes_for_side(axes, &StickSide::Right);
                let sidx = super::util::side_index(&StickSide::Right);
                let side =
                    &mut self.controllers.entry(_cid).or_default().sides[sidx];
                let (raw_x, raw_y) =
                    invert_xy(x0, y0, params.invert_x, params.invert_y);
                side.mouse_filtered.0 += alpha * (raw_x - side.mouse_filtered.0);
                side.mouse_filtered.1 += alpha * (raw_y - side.mouse_filtered.1);
                let (x, y) = side.mouse_filtered;
                let mag_raw = magnitude2d(x, y);
                if mag_raw >= params.deadzone {
                    let base = normalize_after_deadzone(mag_raw, params.deadzone);
                    let mag = Self::fast_gamma(base, params.gamma);
                    if mag > 0.0 {
                        let dir_x = x / mag_raw;
                        let dir_y = y / mag_raw;
                        let speed_px_s = params.max_speed_px_s * mag;
                        let accum = &mut side.mouse_accum;
                        accum.0 += speed_px_s * dir_x * dt_s;
                        accum.1 += speed_px_s * dir_y * dt_s;
                        let dx = accum.0.trunc() as i32;
                        let dy = accum.1.trunc() as i32;
                        if dx != 0 || dy != 0 {
                            print_debug!(
                                "stick mouse: controller={_cid} side=Right raw=({x0:.3},{y0:.3}) filtered=({x:.3},{y:.3}) mag={mag:.3} speed_px_s={speed_px_s:.1} move=({dx},{dy}) accum=({:.3},{:.3})"
                                ,accum.0,accum.1
                            );
                            (sink)(Effect::MouseMove { dx, dy });
                            accum.0 -= dx as f32;
                            accum.1 -= dy as f32;
                        }
                    }
                } else {
                    side.mouse_filtered = (0.0, 0.0);
                    side.mouse_accum = (0.0, 0.0);
                }
            }
        }
    }

    #[inline]
    fn mouse_smoothing_alpha(dt_s: f32, smoothing_window_ms: u64) -> f32 {
        let window_s = (smoothing_window_ms as f32 / 1000.0).max(0.001);
        (dt_s / window_s).clamp(0.18, 0.55)
    }

    #[inline]
    fn fast_gamma(base: f32, gamma: f32) -> f32 {
        let g = gamma.max(0.1);
        if (g - 1.0).abs() < 1e-6 {
            base
        } else if (g - 0.5).abs() < 1e-6 {
            base.sqrt()
        } else if (g - 1.5).abs() < 1e-6 {
            base * base.sqrt()
        } else if (g - 2.0).abs() < 1e-6 {
            base * base
        } else if (g - 3.0).abs() < 1e-6 {
            base * base * base
        } else {
            base.powf(g)
        }
    }

    fn tick_scroll(
        &mut self,
        dt_s: f32,
        sink: &mut impl FnMut(Effect),
        axes_list: &[(ControllerId, [f32; 6])],
        bindings: &CompiledStickRules,
    ) {
        for (cid, axes) in axes_list.iter().cloned() {
            if let Some(StickMode::Scroll(params)) = bindings.left() {
                self.tick_scroll_side(
                    cid,
                    axes,
                    StickSide::Left,
                    params,
                    dt_s,
                    sink,
                );
            }
            if let Some(StickMode::Scroll(params)) = bindings.right() {
                self.tick_scroll_side(
                    cid,
                    axes,
                    StickSide::Right,
                    params,
                    dt_s,
                    sink,
                );
            }
        }
    }

    fn tick_scroll_side(
        &mut self,
        cid: ControllerId,
        axes: [f32; 6],
        side: StickSide,
        params: &gamacros_workspace::ScrollParams,
        dt_s: f32,
        sink: &mut impl FnMut(Effect),
    ) {
        let started_at = std::time::Instant::now();
        let alpha =
            Self::mouse_smoothing_alpha(dt_s, params.runtime.smoothing_window_ms);
        let side_label = match side {
            StickSide::Left => "Left",
            StickSide::Right => "Right",
        };
        let (x0, y0) = axes_for_side(axes, &side);
        let (raw_x, raw_y) = invert_xy(x0, y0, params.invert_x, params.invert_y);
        let sidx = super::util::side_index(&side);
        let side_state = &mut self.controllers.entry(cid).or_default().sides[sidx];
        side_state.scroll_filtered.0 +=
            alpha * (raw_x - side_state.scroll_filtered.0);
        side_state.scroll_filtered.1 +=
            alpha * (raw_y - side_state.scroll_filtered.1);
        let mut x = side_state.scroll_filtered.0;
        let y = side_state.scroll_filtered.1;
        if !params.horizontal {
            x = 0.0;
        }
        let mag_raw = x.abs().max(y.abs());
        if mag_raw <= params.deadzone {
            side_state.scroll_filtered = (0.0, 0.0);
            side_state.scroll_accum = (0.0, 0.0);
            return;
        }

        let base = normalize_after_deadzone(mag_raw, params.deadzone);
        let mag = Self::fast_gamma(base, params.runtime.gamma);
        if mag <= 0.0 {
            return;
        }

        let scale = mag / mag_raw;
        let sx = x * scale;
        let sy = y * scale;
        let (trigger, trigger_boost) = trigger_scroll_boost(axes, params);
        print_debug!(
            "scroll pipeline: controller={cid} side={side_label} dt_s={dt_s:.4} raw=({x0:.3},{y0:.3}) filtered=({x:.3},{y:.3}) mag_raw={mag_raw:.3} base={base:.3} gamma={} mag={mag:.3} scale={scale:.3} speed_lines_s={} alpha={alpha:.3} trigger={trigger:.3} trigger_boost={trigger_boost:.3}",
            params.runtime.gamma,
            params.speed_lines_s
        );
        let accum = &mut side_state.scroll_accum;
        const SCROLL_SPEED_MULTIPLIER: f32 = 6.0;
        let speed = params.speed_lines_s * SCROLL_SPEED_MULTIPLIER * trigger_boost;
        accum.0 += speed * sx * dt_s;
        accum.1 += speed * sy * dt_s;
        let h = f64::from(accum.0);
        let v = f64::from(accum.1);
        if h.abs() >= 0.01 {
            print_debug!(
                "stick scroll: controller={cid} side={side_label} raw=({x0:.3},{y0:.3}) filtered=({x:.3},{y:.3}) mag={mag:.3} accum=({:.3},{:.3}) emit_h={h}",
                accum.0,
                accum.1
            );
            (sink)(Effect::Scroll { h, v: 0.0 });
            accum.0 = 0.0;
        }
        if v.abs() >= 0.01 {
            print_debug!(
                "stick scroll: controller={cid} side={side_label} raw=({x0:.3},{y0:.3}) filtered=({x:.3},{y:.3}) mag={mag:.3} accum=({:.3},{:.3}) emit_v={v}",
                accum.0,
                accum.1
            );
            (sink)(Effect::Scroll { h: 0.0, v });
            accum.1 = 0.0;
        }
        print_debug!(
            "scroll pipeline done: controller={cid} side={side_label} elapsed_us={} remaining_accum=({:.3},{:.3})",
            started_at.elapsed().as_micros(),
            accum.0,
            accum.1
        );
    }

    #[inline]
    pub fn quantize_direction(x: f32, y: f32) -> Option<Direction> {
        let ax = x.abs();
        let ay = y.abs();
        if ax == 0.0 && ay == 0.0 {
            return None;
        }
        if ax > ay {
            if x > 0.0 {
                Some(Direction::Right)
            } else {
                Some(Direction::Left)
            }
        } else if ay > ax {
            if y > 0.0 {
                Some(Direction::Up)
            } else {
                Some(Direction::Down)
            }
        } else if y > 0.0 {
            Some(Direction::Up)
        } else if y < 0.0 {
            Some(Direction::Down)
        } else {
            None
        }
    }

    #[inline]
    pub fn get_direction_key(dir: Direction) -> gamacros_control::Key {
        match dir {
            Direction::Up => gamacros_control::Key::UpArrow,
            Direction::Down => gamacros_control::Key::DownArrow,
            Direction::Left => gamacros_control::Key::LeftArrow,
            Direction::Right => gamacros_control::Key::RightArrow,
        }
    }
}
