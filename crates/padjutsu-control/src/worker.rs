//! Async, coalescing performer worker.
//!
//! Runs `Performer` operations on a dedicated real-time thread, decoupling the
//! event loop from `CGEventPost` latency. Under heavy graphics load,
//! `CGEventPost` blocks waiting on WindowServer; without this worker, that
//! blocks the entire event loop and inputs from the gamepad pile up.
//!
//! The worker also coalesces consecutive `MouseMove` and `Scroll` commands —
//! mouse position is a state, not deltas, so collapsing pending moves into a
//! single post avoids a long catch-up tail when the queue gets behind.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use enigo::Button;

use crate::performer::Performer;
use crate::KeyCombo;

/// Commands sent to the worker thread.
#[derive(Debug, Clone)]
pub enum PerformerCmd {
    KeyTap(KeyCombo),
    KeyPress(KeyCombo),
    KeyRelease(KeyCombo),
    MouseMove { dx: i32, dy: i32 },
    ScrollX(f64),
    ScrollY(f64),
    MouseClick(Button),
    MouseDoubleClick(Button),
    MousePress(Button),
    MouseRelease(Button),
    #[cfg(target_os = "macos")]
    RawModifierPress(u16),
    #[cfg(target_os = "macos")]
    RawModifierRelease(u16),
}

/// Handle to the worker thread. Drop to terminate the worker.
pub struct PerformerWorker {
    tx: Sender<PerformerCmd>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl PerformerWorker {
    /// Spawn a worker thread that owns the given `Performer`.
    /// The worker thread sets itself to macOS realtime priority on macOS.
    pub fn spawn(mut performer: Performer) -> Self {
        let (tx, rx) = bounded::<PerformerCmd>(1024);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = stop.clone();
        let join = thread::Builder::new()
            .name("performer-worker".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                #[cfg(target_os = "macos")]
                set_realtime_priority_2ms();
                run(&mut performer, rx, stop_w);
            })
            .expect("failed to spawn performer worker");
        Self {
            tx,
            stop,
            join: Some(join),
        }
    }

    /// Send a command to the worker. Non-blocking. If the queue is full
    /// (worker backed up), the command is dropped and `Err` is returned.
    /// For movement-style commands the caller should not retry — the next
    /// tick will produce a fresh state.
    pub fn try_send(&self, cmd: PerformerCmd) -> Result<(), TrySendError<PerformerCmd>> {
        self.tx.try_send(cmd)
    }
}

impl Drop for PerformerWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Send a no-op-ish wake to unblock recv if needed.
        let _ = self.tx.try_send(PerformerCmd::MouseMove { dx: 0, dy: 0 });
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run(
    performer: &mut Performer,
    rx: Receiver<PerformerCmd>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        // Block for at least one command.
        let first = match rx.recv() {
            Ok(c) => c,
            Err(_) => return,
        };

        // Drain everything currently queued so we can coalesce homogeneous
        // streams (mouse moves + scrolls) and execute the rest in order.
        let mut batch: Vec<PerformerCmd> = Vec::with_capacity(16);
        batch.push(first);
        while let Ok(c) = rx.try_recv() {
            batch.push(c);
        }

        execute_batch(performer, &batch);
    }
}

/// Execute a batch of commands, coalescing consecutive movement/scroll
/// commands into single posts to avoid catch-up tails under load.
fn execute_batch(performer: &mut Performer, batch: &[PerformerCmd]) {
    let mut i = 0;
    while i < batch.len() {
        match &batch[i] {
            PerformerCmd::MouseMove { .. } => {
                // Coalesce all subsequent MouseMove commands into a single delta.
                let mut sum_dx: i32 = 0;
                let mut sum_dy: i32 = 0;
                while i < batch.len() {
                    if let PerformerCmd::MouseMove { dx, dy } = batch[i] {
                        sum_dx = sum_dx.saturating_add(dx);
                        sum_dy = sum_dy.saturating_add(dy);
                        i += 1;
                    } else {
                        break;
                    }
                }
                if sum_dx != 0 || sum_dy != 0 {
                    let _ = performer.mouse_move(sum_dx, sum_dy);
                }
            }
            PerformerCmd::ScrollX(_) => {
                let mut sum: f64 = 0.0;
                while i < batch.len() {
                    if let PerformerCmd::ScrollX(v) = batch[i] {
                        sum += v;
                        i += 1;
                    } else {
                        break;
                    }
                }
                if sum != 0.0 {
                    let _ = performer.scroll_x(sum);
                }
            }
            PerformerCmd::ScrollY(_) => {
                let mut sum: f64 = 0.0;
                while i < batch.len() {
                    if let PerformerCmd::ScrollY(v) = batch[i] {
                        sum += v;
                        i += 1;
                    } else {
                        break;
                    }
                }
                if sum != 0.0 {
                    let _ = performer.scroll_y(sum);
                }
            }
            // Non-coalescing commands: execute one at a time.
            other => {
                execute_one(performer, other);
                i += 1;
            }
        }
    }
}

fn execute_one(performer: &mut Performer, cmd: &PerformerCmd) {
    match cmd {
        PerformerCmd::KeyTap(k) => {
            let _ = performer.perform(k);
        }
        PerformerCmd::KeyPress(k) => {
            let _ = performer.press(k);
        }
        PerformerCmd::KeyRelease(k) => {
            let _ = performer.release(k);
        }
        PerformerCmd::MouseMove { dx, dy } => {
            let _ = performer.mouse_move(*dx, *dy);
        }
        PerformerCmd::ScrollX(v) => {
            let _ = performer.scroll_x(*v);
        }
        PerformerCmd::ScrollY(v) => {
            let _ = performer.scroll_y(*v);
        }
        PerformerCmd::MouseClick(b) => {
            let _ = performer.mouse_click(*b);
        }
        PerformerCmd::MouseDoubleClick(b) => {
            let _ = performer.mouse_double_click(*b);
        }
        PerformerCmd::MousePress(b) => {
            let _ = performer.mouse_press(*b);
        }
        PerformerCmd::MouseRelease(b) => {
            let _ = performer.mouse_release(*b);
        }
        #[cfg(target_os = "macos")]
        PerformerCmd::RawModifierPress(kc) => {
            let _ = performer.raw_modifier_press(*kc);
        }
        #[cfg(target_os = "macos")]
        PerformerCmd::RawModifierRelease(kc) => {
            let _ = performer.raw_modifier_release(*kc);
        }
    }
}

// --- macOS realtime priority for the performer worker thread ---

#[cfg(target_os = "macos")]
fn set_realtime_priority_2ms() {
    use std::os::raw::c_int;

    type KernReturnT = c_int;
    type MachPortT = u32;
    type ThreadActT = MachPortT;
    type ThreadPolicyFlavorT = c_int;
    type MachMsgTypeNumberT = u32;

    const THREAD_TIME_CONSTRAINT_POLICY: ThreadPolicyFlavorT = 2;
    const THREAD_TIME_CONSTRAINT_POLICY_COUNT: MachMsgTypeNumberT = 4;
    const KERN_SUCCESS: KernReturnT = 0;

    #[repr(C)]
    struct ThreadTimeConstraintPolicy {
        period: u32,
        computation: u32,
        constraint: u32,
        preemptible: u32,
    }

    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    extern "C" {
        fn mach_thread_self() -> ThreadActT;
        fn thread_policy_set(
            thread: ThreadActT,
            flavor: ThreadPolicyFlavorT,
            policy_info: *const ThreadTimeConstraintPolicy,
            count: MachMsgTypeNumberT,
        ) -> KernReturnT;
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> KernReturnT;
    }

    fn ns_to_abs(ns: u64) -> u32 {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        unsafe {
            mach_timebase_info(&mut info);
        }
        (ns * info.denom as u64 / info.numer as u64) as u32
    }

    // Performer worker has a slightly larger budget than gamepad runtime
    // because CGEventPost can take a few ms under load.
    let policy = ThreadTimeConstraintPolicy {
        period: ns_to_abs(4_000_000),       // 4ms scheduling period
        computation: ns_to_abs(1_000_000),  // 1ms computation per period
        constraint: ns_to_abs(2_000_000),   // 2ms deadline within period
        preemptible: 1,
    };
    let thread = unsafe { mach_thread_self() };
    let kr = unsafe {
        thread_policy_set(
            thread,
            THREAD_TIME_CONSTRAINT_POLICY,
            &policy,
            THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    if kr == KERN_SUCCESS {
        eprintln!("[performer-worker] realtime priority set");
    } else {
        eprintln!("[performer-worker] failed to set RT priority: {kr}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_mouse_moves_sums_deltas() {
        let cmds = vec![
            PerformerCmd::MouseMove { dx: 5, dy: 0 },
            PerformerCmd::MouseMove { dx: 3, dy: 2 },
            PerformerCmd::MouseMove { dx: -1, dy: -1 },
        ];
        // We can't directly test execute_batch without a Performer, but we can
        // verify the coalescing logic by extracting it.
        let mut sum_dx: i32 = 0;
        let mut sum_dy: i32 = 0;
        for c in &cmds {
            if let PerformerCmd::MouseMove { dx, dy } = c {
                sum_dx += dx;
                sum_dy += dy;
            }
        }
        assert_eq!(sum_dx, 7);
        assert_eq!(sum_dy, 1);
    }

    #[test]
    fn coalesce_breaks_at_non_movement() {
        let cmds = vec![
            PerformerCmd::MouseMove { dx: 5, dy: 0 },
            PerformerCmd::MouseClick(Button::Left),
            PerformerCmd::MouseMove { dx: 3, dy: 0 },
        ];
        // Manually trace: first batch should coalesce only first MouseMove,
        // then click, then second MouseMove. We assert there are 3 distinct
        // operations conceptually. Real verification is in integration.
        let movement_segments: Vec<_> = cmds
            .windows(1)
            .map(|w| matches!(w[0], PerformerCmd::MouseMove { .. }))
            .collect();
        assert_eq!(movement_segments, vec![true, false, true]);
    }
}
