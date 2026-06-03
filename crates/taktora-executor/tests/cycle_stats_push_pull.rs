//! `TEST_0193` — push and pull stat paths agree (`REQ_0103`), with exact `took`
//! via an injected [`MockClock`].
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{
    ControlFlow, CycleObservation, Executor, MockClock, Observer, item_with_triggers,
};

const TOOK_NS: u64 = 1_000_000; // 1 ms exact body cost

#[derive(Default)]
struct Recorder {
    count: AtomicU64,
    last_index: Mutex<Option<u64>>,
    last_took: AtomicU64,
}
impl Observer for Recorder {
    fn on_cycle_stats(&self, o: &CycleObservation) {
        self.count.fetch_add(1, Ordering::Relaxed);
        *self.last_index.lock().unwrap() = Some(o.cycle_index);
        // Healthy cycle: took is always Some here; unwrap to the sentinel only
        // defensively (a None would fail the exact assertion below).
        self.last_took
            .store(o.took_ns.unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

#[test]
fn push_count_matches_cycles_and_pull_reflects_samples() {
    let clock = MockClock::new();
    let body_clock = clock.clone();
    let rec = Arc::new(Recorder::default());
    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(rec.clone())
        .stats_window(1024)
        .clock(Arc::new(clock))
        .build()
        .expect("build");
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |_ctx| {
            body_clock.advance(TOOK_NS);
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add");

    exec.run_n(20).expect("run");

    // Push: one on_cycle_stats per cycle; deterministic run_n(20) => 20.
    assert_eq!(rec.count.load(Ordering::Relaxed), 20, "one push per cycle");
    assert_eq!(
        *rec.last_index.lock().unwrap(),
        Some(19),
        "cycle_index 0..=19"
    );
    // Push value is exact: the body advanced the clock by exactly TOOK_NS.
    assert_eq!(
        rec.last_took.load(Ordering::Relaxed),
        TOOK_NS,
        "pushed took_ns must equal the injected body cost exactly"
    );

    // Pull agrees with push: snapshot max_ns equals the same exact took.
    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1);
    assert_eq!(
        snap.per_task[0].max_ns, TOOK_NS,
        "pull max_ns must agree with the pushed took exactly"
    );
}
