//! `REQ_0107` — `cycle_index` is monotonic and `on_cycle_stats` fires on EVERY
//! scan attempt INCLUDING faulted/fault-routed scans (`FEAT_0038` cross-layer
//! join).
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{CycleObservation, Executor, ItemFlow, Observer, item_with_triggers};

#[derive(Default)]
struct IdxRec {
    obs: Mutex<Vec<(u64, Option<u64>, bool)>>,
} // (cycle_index, took_ns, faulted)
impl Observer for IdxRec {
    fn on_cycle_stats(&self, o: &CycleObservation) {
        self.obs
            .lock()
            .unwrap()
            .push((o.cycle_index, o.took_ns, o.faulted));
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
            Ok(ItemFlow::Continue)
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
    for (pos, &(idx, _, _)) in obs.iter().enumerate() {
        assert_eq!(
            idx, pos as u64,
            "cycle_index desynced at position {pos}: {obs:?}"
        );
    }
    // Cycle 0 ran the body (not faulted, real took); the post-execute budget
    // breach faults the task, so every later wakeup is fault-routed: REQ_0107
    // says it still emits, and the REQ_0103/REQ_0267 contract says a faulted
    // scan is *distinguishable* — `faulted == true` and `took_ns == None`, not
    // an ambiguous zero.
    assert!(!obs[0].2, "cycle 0 ran the body, not faulted");
    assert!(
        matches!(obs[0].1, Some(t) if t > 0),
        "cycle 0 should carry a real took, got {:?}",
        obs[0].1
    );
    assert!(
        obs[1..]
            .iter()
            .all(|&(_, took, faulted)| faulted && took.is_none()),
        "faulted scans must be flagged faulted with took_ns == None: {obs:?}"
    );
}
