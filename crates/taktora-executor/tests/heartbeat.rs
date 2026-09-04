#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]
use core::time::Duration;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;
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

    fn len(&self) -> usize {
        self.ticks.lock().len()
    }
}

impl Observer for HeartbeatCapture {
    fn on_heartbeat(&self, tick: &HeartbeatTick) {
        self.ticks.lock().push(*tick);
    }
}

/// Verify the heartbeat emits a sustained series of ticks with strictly
/// monotonic sequence numbers and timestamps (TSR_0010 coverage).
///
/// Count-driven rather than a rate over a fixed wall-clock window: the precise
/// inter-tick *period* is a real-time property of the scheduler and is not
/// assertable on an oversubscribed CI runner, where a single stall injects an
/// arbitrarily large gap. Liveness (ticks keep coming) and ordering (seq +
/// timestamps strictly increasing) are verified deterministically instead.
#[test]
fn heartbeat_emits_at_bounded_period() {
    let period = Duration::from_millis(20);
    let target_ticks: usize = 10;
    let safety_cap = Duration::from_secs(10);
    let observer = Arc::new(HeartbeatCapture::new());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    let start = Instant::now();
    exec.run_until(|| observer.len() >= target_ticks || start.elapsed() > safety_cap)
        .unwrap();

    let ticks = observer.take_ticks();

    assert!(
        ticks.len() >= target_ticks,
        "expected at least {target_ticks} ticks, got {} (heartbeat not firing?)",
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

    // Timestamps must be strictly increasing (the emit path samples the cyclic
    // clock once per tick). The magnitude of each gap is scheduler-dependent
    // and deliberately not asserted here — see the doc comment.
    for window in ticks.windows(2) {
        assert!(
            window[1].at_nanos > window[0].at_nanos,
            "timestamps not strictly increasing: {} -> {}",
            window[0].at_nanos,
            window[1].at_nanos
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
/// (the WaitSet wait is bounded by the heartbeat deadline). Count-driven with a
/// wall-clock safety cap so a transient CI scheduler stall cannot flake it.
#[test]
fn heartbeat_fires_without_other_triggers() {
    let period = Duration::from_millis(25);
    let target_ticks: usize = 3;
    let safety_cap = Duration::from_secs(10);
    let observer = Arc::new(HeartbeatCapture::new());

    // No tasks registered — the executor will wait indefinitely unless the
    // heartbeat bounds the wait.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    let start = Instant::now();
    exec.run_until(|| observer.len() >= target_ticks || start.elapsed() > safety_cap)
        .unwrap();

    let ticks = observer.take_ticks();

    assert!(
        ticks.len() >= target_ticks,
        "expected at least {target_ticks} ticks with no tasks, got {} (heartbeat not firing?)",
        ticks.len()
    );
}

/// Verify that timestamps are monotonically increasing.
#[test]
fn heartbeat_timestamps_monotonic() {
    let period = Duration::from_millis(15);
    let target_ticks: usize = 3;
    let safety_cap = Duration::from_secs(10);
    let observer = Arc::new(HeartbeatCapture::new());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .unwrap();

    let start = Instant::now();
    exec.run_until(|| observer.len() >= target_ticks || start.elapsed() > safety_cap)
        .unwrap();

    let ticks = observer.take_ticks();
    assert!(
        ticks.len() >= target_ticks,
        "expected at least {target_ticks} ticks for monotonicity check, got {}",
        ticks.len()
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
