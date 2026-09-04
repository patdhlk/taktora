#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
use core::time::Duration;
use parking_lot::Mutex;
use std::sync::Arc;
use taktora_executor::{Executor, HeartbeatTick, ItemFlow, Observer, item_with_triggers};

/// Captures heartbeat ticks for verification.
#[derive(Clone)]
struct HeartbeatCapture {
    ticks: Arc<Mutex<Vec<HeartbeatTick>>>,
}

impl HeartbeatCapture {
    fn new() -> Self {
        Self {
            ticks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn take_ticks(&self) -> Vec<HeartbeatTick> {
        let mut guard = self.ticks.lock();
        core::mem::take(&mut *guard)
    }
}

impl Observer for HeartbeatCapture {
    fn on_heartbeat(&self, tick: &HeartbeatTick) {
        self.ticks.lock().push(*tick);
    }
}

/// Verify that the heartbeat emits at least the expected number of ticks and
/// that the inter-tick gap is bounded by a reasonable multiple of the period
/// (TSR_0006 latency-test intent, TSR_0010 coverage).
#[test]
fn heartbeat_emits_at_bounded_period() {
    let period = Duration::from_millis(20);
    let run_duration = Duration::from_millis(300);
    let observer = Arc::new(HeartbeatCapture::new());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    exec.run_for(run_duration).unwrap();

    let ticks = observer.take_ticks();

    // We should get at least run_duration / period ticks, minus a small
    // tolerance for setup and the final partial period.
    let expected_min = (run_duration.as_millis() / period.as_millis()) - 1;
    assert!(
        ticks.len() as u128 >= expected_min,
        "expected at least {expected_min} ticks in {run_duration:?}, got {}",
        ticks.len()
    );

    // Sequence numbers must be strictly monotonic starting at 1.
    for (i, tick) in ticks.iter().enumerate() {
        let expected_seq = (i + 1) as u64;
        assert_eq!(
            tick.seq, expected_seq,
            "tick {i}: seq mismatch (expected {expected_seq}, got {})",
            tick.seq
        );
    }

    // Inter-tick gaps must be bounded. Allow generous tolerance for CI timing
    // (period * 2.5).
    let max_gap_nanos = u64::try_from(period.as_nanos()).unwrap_or(u64::MAX) * 5 / 2;
    for window in ticks.windows(2) {
        let gap = window[1].at_nanos.saturating_sub(window[0].at_nanos);
        assert!(
            gap <= max_gap_nanos,
            "inter-tick gap {gap} ns exceeds bound {max_gap_nanos} ns (period: {period:?})"
        );
    }
}

/// Verify that an executor without a configured heartbeat emits no ticks.
#[test]
fn no_heartbeat_when_unconfigured() {
    let observer = Arc::new(HeartbeatCapture::new());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    // A cyclic task provides the wakeup source so `run_n` terminates; with no
    // heartbeat configured, `on_heartbeat` must never fire.
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        |_| Ok(ItemFlow::Continue),
    ))
    .unwrap();

    exec.run_n(3).unwrap();

    let ticks = observer.take_ticks();
    assert_eq!(
        ticks.len(),
        0,
        "expected zero ticks when heartbeat is not configured"
    );
}

/// Verify that the heartbeat fires even when no other triggers are active
/// (the WaitSet wait is bounded by the heartbeat deadline).
#[test]
fn heartbeat_fires_without_other_triggers() {
    let period = Duration::from_millis(25);
    let run_duration = Duration::from_millis(150);
    let observer = Arc::new(HeartbeatCapture::new());

    // No tasks registered — the executor will wait indefinitely unless the
    // heartbeat bounds the wait.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    exec.run_for(run_duration).unwrap();

    let ticks = observer.take_ticks();

    // Should still get ticks even with no tasks.
    let expected_min = (run_duration.as_millis() / period.as_millis()) - 1;
    assert!(
        ticks.len() as u128 >= expected_min,
        "expected at least {expected_min} ticks with no tasks, got {}",
        ticks.len()
    );
}

/// Verify that timestamps are monotonically increasing.
#[test]
fn heartbeat_timestamps_monotonic() {
    let period = Duration::from_millis(15);
    let observer = Arc::new(HeartbeatCapture::new());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    exec.run_for(Duration::from_millis(100)).unwrap();

    let ticks = observer.take_ticks();
    assert!(
        !ticks.is_empty(),
        "expected at least one tick for monotonicity check"
    );

    for window in ticks.windows(2) {
        assert!(
            window[1].at_nanos > window[0].at_nanos,
            "timestamps not monotonic: {} -> {}",
            window[0].at_nanos,
            window[1].at_nanos
        );
    }
}
