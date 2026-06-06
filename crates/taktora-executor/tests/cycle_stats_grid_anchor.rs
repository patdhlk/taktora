//! `REQ_0106`: in `Grid` mode the per-task lateness grid anchors at the
//! first dispatch's **nominal slot**, not its observed dispatch instant. A
//! late first dispatch (process started under load) must report its real
//! startup delay as positive lateness — anchoring at the observed instant
//! instead bakes that delay in as a permanent negative floor on every
//! later on-grid cycle (found on the Pi5 rig: −110 µs…−792 µs constant).
//!
//! The cyclic scheduling clock is scripted (`CyclicClock` seam), so the
//! grid arithmetic is exact; only the wake cadence is real.

// Linux-only: the call sequence into the scripted `CyclicClock` is only
// deterministic on the production timerfd path (loop entry + one read per
// wake). The non-Linux Grid fallback also reads the clock to self-compute
// epoll timeouts, which shifts the script per platform/wake — and it is
// not a real-time target. The ferry arithmetic stays covered everywhere
// by the `GridTimer` unit tests (grid.rs).
#![cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{
    ControlFlow, CycleObservation, CyclicClock, DispatchMode, Executor, Observer,
    item_with_triggers,
};

/// Scripted scheduling clock: loop entry reads 0 (grid epoch), the first
/// wake reads 1.7 ms (0.7 ms past the task's first nominal slot at 1 ms),
/// the second wake reads 2.0 ms (exactly on the next slot). Saturates on
/// the last value so stray extra reads stay harmless.
struct ScriptClock {
    values: Vec<u64>,
    next: AtomicUsize,
}

impl CyclicClock for ScriptClock {
    fn now_nanos(&self) -> u64 {
        let i = self.next.fetch_add(1, Ordering::SeqCst);
        self.values[i.min(self.values.len() - 1)]
    }
}

#[derive(Default)]
struct Recorder {
    samples: Mutex<Vec<CycleObservation>>,
}

impl Observer for Recorder {
    fn on_cycle_stats(&self, obs: &CycleObservation) {
        self.samples.lock().unwrap().push(obs.clone());
    }
}

#[test]
fn late_first_dispatch_reports_startup_delay_not_zero() {
    let recorder = Arc::new(Recorder::default());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .dispatch_mode(DispatchMode::Grid)
        .cyclic_clock(Arc::new(ScriptClock {
            values: vec![0, 1_700_000, 2_000_000],
            next: AtomicUsize::new(0),
        }))
        .observer(Arc::clone(&recorder) as Arc<dyn Observer>)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        |_| Ok(ControlFlow::Continue),
    ))
    .expect("add task");

    exec.run_n(2).expect("run_n");

    let samples = recorder.samples.lock().unwrap();
    assert!(
        samples.len() >= 2,
        "expected 2 observations, got {}",
        samples.len()
    );

    // First dispatch happened 0.7 ms after its nominal slot (scripted): the
    // grid must anchor at the slot, so the startup delay is REPORTED, not
    // erased to 0 / inverted into a permanent negative floor. Exact: the
    // anchor back-dates the first observed `pre` by the scripted 0.7 ms, so
    // sample 0's lateness is the scripted delay itself, no real-clock term.
    assert_eq!(
        samples[0].lateness_ns,
        Some(700_000),
        "first-cycle lateness must equal the dispatch's delay past its \
         nominal grid slot"
    );

    // No skip-realign happened in this script.
    assert_eq!(samples[0].skipped_slots, 0);
    assert_eq!(samples[1].skipped_slots, 0);
}
