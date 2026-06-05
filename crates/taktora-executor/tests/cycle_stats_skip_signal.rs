//! `REQ_0840` layer-2: a real dispatcher starvation in `Grid` mode reaches
//! telemetry as `skipped_slots` and re-anchors the lateness grid. Real clocks
//! (the starvation is genuine: `worker_threads(0)` runs the sleeping body on
//! the dispatch thread), so assertions are deliberately loose bounds — the
//! exact carry arithmetic is pinned by the `GridTimer` unit tests.

// The whole file is Linux-only: layer 2 of TEST_0853 needs the production
// absolute-grid timerfd path. The non-Linux Grid fallback (ms-rounded epoll
// timeout) is not a real-time target — a loaded CI runner can stall the
// final wakes, leaving the last sample mid-starvation with its re-anchor
// landing after `run_n` returns, so no tail bound holds there. The ferry
// mechanics stay covered everywhere by the GridTimer unit tests (grid.rs).
#![cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{
    ControlFlow, CycleObservation, DispatchMode, Executor, Observer, item_with_triggers,
};

const PERIOD: Duration = Duration::from_millis(50);
const PERIOD_NS: i64 = 50_000_000;

#[derive(Default)]
struct Recorder {
    samples: Mutex<Vec<(Option<i64>, u32)>>,
}

impl Observer for Recorder {
    fn on_cycle_stats(&self, obs: &CycleObservation) {
        self.samples
            .lock()
            .unwrap()
            .push((obs.lateness_ns, obs.skipped_slots));
    }
}

#[test]
fn starved_dispatch_signals_skipped_slots_and_re_anchors() {
    let recorder = Arc::new(Recorder::default());
    let n = Arc::new(AtomicU64::new(0));

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .dispatch_mode(DispatchMode::Grid)
        .observer(Arc::clone(&recorder) as Arc<dyn Observer>)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(PERIOD);
            Ok(())
        },
        move |_ctx| {
            // Cycle index 2 (the third scan) starves the dispatch thread for ~2.6 periods, forcing
            // a GridTimer skip-realign; every other cycle returns instantly.
            if n.fetch_add(1, Ordering::Relaxed) == 2 {
                std::thread::sleep(Duration::from_millis(130));
            }
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(8).expect("run_n");

    let samples = recorder.samples.lock().unwrap().clone();
    // run_n counts WaitSet wakeups, not observations: on the non-Linux Grid
    // fallback (ms-rounded epoll timeout, no master timerfd) a wake can find
    // no slot due and emit nothing, so the exact count is platform-dependent.
    // A lower bound keeps the structural assertions meaningful on both legs.
    assert!(
        samples.len() >= 4,
        "expected at least 4 observations, got {}",
        samples.len()
    );

    // The realign's abandoned-slot count arrives on the dispatch AFTER the
    // starved one (backward-looking, REQ_0840).
    let total_skipped: u32 = samples.iter().map(|(_, s)| s).sum();
    assert!(
        total_skipped >= 1,
        "the 130 ms starvation must signal at least one skipped slot, got {samples:?}"
    );
    assert_eq!(samples[0].1, 0, "first cycle never reports skips");

    // The starvation itself spikes lateness past a period...
    let max_lateness = samples.iter().filter_map(|(l, _)| *l).max().unwrap();
    assert!(
        max_lateness >= PERIOD_NS,
        "starved cycle must read >= one period late, got {max_lateness}"
    );
    // ...and the signal re-anchors: the run's tail is back under half a
    // period (a permanent +N*PERIOD residue would mean the signal was lost).
    let tail = samples.last().unwrap().0.unwrap();
    assert!(
        tail.abs() < PERIOD_NS / 2,
        "post-skip lateness must re-anchor, got tail {tail}"
    );
}
