//! `REQ_0107` — `cycle_index` is monotonic and `on_cycle_stats` fires on EVERY
//! scan attempt INCLUDING faulted/fault-routed scans (`FEAT_0038` cross-layer
//! join).
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{ControlFlow, CycleObservation, Executor, Observer, item_with_triggers};

#[derive(Default)]
struct IdxRec {
    obs: Mutex<Vec<(u64, u64)>>,
} // (cycle_index, took_ns)
impl Observer for IdxRec {
    fn on_cycle_stats(&self, o: &CycleObservation) {
        self.obs.lock().unwrap().push((o.cycle_index, o.took_ns));
    }
}

#[test]
fn cycle_index_contiguous_including_faulted_scans() {
    let rec = Arc::new(IdxRec::default());
    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(rec.clone())
        .stats_window(1024)
        .build()
        .expect("build");
    // 5ms interval, 2ms budget. Body always sleeps ~6ms -> breaches budget on
    // cycle 0 (faults the task). Cycle 0 runs (normal path). Every later
    // wakeup is fault-routed -> must STILL increment cycle_index + emit.
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(5));
            d.budget(Duration::from_millis(2));
            Ok(())
        },
        move |_ctx| {
            std::thread::sleep(Duration::from_millis(6));
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add");

    exec.run_n(6).expect("run");

    let obs = rec.obs.lock().unwrap().clone();
    // One emission per scan attempt, fault-routed cycles included.
    assert_eq!(
        obs.len(),
        6,
        "expected 6 emissions (one per attempt), got {}",
        obs.len()
    );
    // THE KEY ASSERTION: gap-free monotonic cycle_index 0,1,2,3,4,5.
    for (pos, &(idx, _)) in obs.iter().enumerate() {
        assert_eq!(
            idx, pos as u64,
            "cycle_index desynced at position {pos}: {obs:?}"
        );
    }
    // Cycle 0 ran (took>0); faulted cycles report took_ns==0 (poison-safe None).
    assert!(obs[0].1 > 0, "cycle 0 should have a real took");
    assert!(
        obs[1..].iter().all(|&(_, took)| took == 0),
        "faulted scans report took 0"
    );
}
