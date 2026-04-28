//! Latency and throughput metrics for the gamepad backend.
//!
//! Tracks deltas between sequential AxisMotion events on the same axis — this
//! is a proxy for the device HID poll period as observed by the backend, the
//! main number that should improve when switching from SDL2/GameController
//! to IOKit/IOHIDManager directly.
//!
//! Enable by setting `PADJUTSU_METRICS=1` env var. Output goes to stderr
//! every 5 seconds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::types::Axis;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    let on = std::env::var("PADJUTSU_METRICS")
        .ok()
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        eprintln!("[padjutsu-metrics] enabled (PADJUTSU_METRICS=1)");
    }
}

#[inline]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Fixed-bucket histogram for sub-ms to ~30ms latencies (HID poll period range).
/// Buckets in microseconds. Anything beyond 30ms is considered a "pause" between
/// stick movements (user not moving the stick) and excluded from poll-period stats.
const BUCKETS_US: &[u64] = &[
    250, 500, 1_000, 2_000, 3_000, 4_000, 6_000, 8_000, 10_000, 12_000, 14_000, 16_000, 20_000, 25_000, 30_000,
];
const POLL_CUTOFF_US: u64 = 30_000;

#[derive(Default)]
struct Histogram {
    counts: [u64; 16],
    sum_us: u128,
    max_us: u64,
    min_us: u64,
    n: u64,
    pauses: u64, // events with dt > POLL_CUTOFF_US — user not actively moving
}

impl Histogram {
    fn observe(&mut self, dt: Duration) {
        let us = dt.as_micros() as u64;
        if us > POLL_CUTOFF_US {
            // User stopped moving for a while — this delta is dominated by their
            // idle time, not by HID throughput. Track as "pause" but don't pollute
            // poll-period stats.
            self.pauses += 1;
            return;
        }
        self.n += 1;
        self.sum_us += us as u128;
        if self.n == 1 || us > self.max_us {
            self.max_us = us;
        }
        if self.n == 1 || us < self.min_us {
            self.min_us = us;
        }
        let mut idx = BUCKETS_US.len();
        for (i, &b) in BUCKETS_US.iter().enumerate() {
            if us < b {
                idx = i;
                break;
            }
        }
        self.counts[idx] += 1;
    }

    /// Approximate percentile from bucketed counts. Returns upper bound of bucket.
    fn percentile_us(&self, p: f64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let target = ((self.n as f64) * p).ceil() as u64;
        let mut acc = 0u64;
        for (i, &c) in self.counts.iter().enumerate() {
            acc += c;
            if acc >= target {
                if i < BUCKETS_US.len() {
                    return BUCKETS_US[i];
                }
                return self.max_us;
            }
        }
        self.max_us
    }

    fn reset(&mut self) {
        *self = Histogram::default();
    }

    fn pauses(&self) -> u64 {
        self.pauses
    }

    fn avg_us(&self) -> u64 {
        if self.n == 0 {
            0
        } else {
            (self.sum_us / self.n as u128) as u64
        }
    }
}

pub struct Metrics {
    last_axis_event: [Option<Instant>; 6],
    axis_delta: [Histogram; 6],
    broadcast_cost: Histogram,
    button_events: u64,
    axis_events: u64,
    last_report: Instant,
    report_interval: Duration,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            last_axis_event: Default::default(),
            axis_delta: Default::default(),
            broadcast_cost: Histogram::default(),
            button_events: 0,
            axis_events: 0,
            last_report: Instant::now(),
            report_interval: Duration::from_secs(5),
        }
    }
}

#[inline]
fn axis_idx(a: Axis) -> usize {
    match a {
        Axis::LeftX => 0,
        Axis::LeftY => 1,
        Axis::RightX => 2,
        Axis::RightY => 3,
        Axis::LeftTrigger => 4,
        Axis::RightTrigger => 5,
    }
}

fn axis_label(idx: usize) -> &'static str {
    match idx {
        0 => "LX",
        1 => "LY",
        2 => "RX",
        3 => "RY",
        4 => "LT",
        5 => "RT",
        _ => "?",
    }
}

impl Metrics {
    pub fn record_axis(&mut self, axis: Axis, t: Instant) {
        if !is_enabled() {
            return;
        }
        self.axis_events += 1;
        let i = axis_idx(axis);
        if let Some(prev) = self.last_axis_event[i] {
            let dt = t.saturating_duration_since(prev);
            self.axis_delta[i].observe(dt);
        }
        self.last_axis_event[i] = Some(t);
    }

    pub fn record_button(&mut self) {
        if !is_enabled() {
            return;
        }
        self.button_events += 1;
    }

    pub fn record_broadcast_cost(&mut self, dt: Duration) {
        if !is_enabled() {
            return;
        }
        self.broadcast_cost.observe(dt);
    }

    pub fn maybe_report(&mut self) {
        if !is_enabled() {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_report) < self.report_interval {
            return;
        }
        self.report(now);
    }

    fn report(&mut self, now: Instant) {
        eprintln!(
            "[padjutsu-metrics] interval={}s axis_events={} button_events={}",
            self.report_interval.as_secs(),
            self.axis_events,
            self.button_events,
        );
        for i in 0..6 {
            let h = &self.axis_delta[i];
            if h.n == 0 && h.pauses() == 0 {
                continue;
            }
            eprintln!(
                "[padjutsu-metrics]   axis {} dt: active_n={} pauses={} min={}us avg={}us p50<={}us p95<={}us p99<={}us max={}us",
                axis_label(i),
                h.n,
                h.pauses(),
                h.min_us,
                h.avg_us(),
                h.percentile_us(0.50),
                h.percentile_us(0.95),
                h.percentile_us(0.99),
                h.max_us,
            );
        }
        if self.broadcast_cost.n > 0 {
            let h = &self.broadcast_cost;
            eprintln!(
                "[padjutsu-metrics]   broadcast cost: n={} avg={}us p95<={}us p99<={}us max={}us",
                h.n,
                h.avg_us(),
                h.percentile_us(0.95),
                h.percentile_us(0.99),
                h.max_us,
            );
        }
        // Reset window so each report covers only the last interval.
        for h in self.axis_delta.iter_mut() {
            h.reset();
        }
        self.broadcast_cost.reset();
        self.button_events = 0;
        self.axis_events = 0;
        self.last_report = now;
    }
}
