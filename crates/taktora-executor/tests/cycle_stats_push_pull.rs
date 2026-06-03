//! `TEST_0193` — push and pull stat paths agree (`REQ_0103`).
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{ControlFlow, CycleObservation, Executor, Observer, item_with_triggers};

#[derive(Default)]
struct Recorder {
    count: AtomicU64,
    last_index: Mutex<Option<u64>>,
}
impl Observer for Recorder {
    fn on_cycle_stats(&self, o: &CycleObservation) {
        self.count.fetch_add(1, Ordering::Relaxed);
        *self.last_index.lock().unwrap() = Some(o.cycle_index);
    }
}

#[test]
fn push_count_matches_cycles_and_pull_reflects_samples() {
    let rec = Arc::new(Recorder::default());
    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(rec.clone())
        .stats_window(1024)
        .build()
        .expect("build");
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(2));
            Ok(())
        },
        move |_ctx| {
            std::thread::sleep(Duration::from_millis(1));
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

    // Pull agrees: snapshot reflects the ~1ms body.
    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1);
    assert!(
        snap.per_task[0].max_ns >= 1,
        "max execute duration should be > 0 after ~1ms body"
    );
}
