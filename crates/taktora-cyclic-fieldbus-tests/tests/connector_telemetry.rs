//! End-to-end contract for the `FEAT_0038` connector telemetry seam: a
//! fallible reference `CyclicFieldbus` folds `ConnectorCycleStats` inside
//! `exchange()` and fires the push observer on every path, proving
//! emit-on-error, poison-safe folding, and the pull snapshot together.

use core::cell::RefCell;

use taktora_cyclic_fieldbus::{
    ConnectorCycleObserver, CycleObservation, CycleOutcome, CycleQuality, CyclicFieldbus,
    CyclicFieldbusTelemetry, Validity,
};
use taktora_stats::{ConnectorCycleSnapshot, ConnectorCycleStats};

const N: usize = 2;
const S: usize = 4;
const W: usize = 64;

/// Captures every pushed observation (single-threaded test, `RefCell` ok).
#[derive(Default)]
struct Recorder {
    seen: RefCell<Vec<CycleObservation>>,
}

impl ConnectorCycleObserver for Recorder {
    fn on_connector_cycle(&self, obs: &CycleObservation) {
        self.seen.borrow_mut().push(*obs);
    }
}

/// Reference fallible bus. Each `exchange()` either completes (synthetic
/// durations) or hard-faults (durations `None`, returns `Err`); both paths
/// fold telemetry and push an observation.
#[derive(Debug)]
struct BusFault;

struct RefBus {
    stats: ConnectorCycleStats<N, S, W>,
    recorder: Recorder,
    fault_next: bool,
    stale: [bool; N],
}

impl RefBus {
    fn new() -> Self {
        Self {
            stats: ConnectorCycleStats::new(1000),
            recorder: Recorder::default(),
            fault_next: false,
            stale: [false; N],
        }
    }
}

impl CyclicFieldbus for RefBus {
    type Routing = usize;
    type Error = BusFault;

    async fn exchange(&mut self) -> Result<CycleQuality, Self::Error> {
        let faulted = self.fault_next;
        self.fault_next = false;

        // Driver-supplied durations: None on a hard fault (REQ_0267).
        let (wire_round_ns, phase_wait_ns) = if faulted {
            (None, None)
        } else {
            (Some(1000u32), Some(500u32))
        };

        let stale_count = u16::try_from(self.stale.iter().filter(|s| **s).count())
            .expect("device count fits u16");
        let all_fresh = stale_count == 0;
        let wc_ok = !faulted && all_fresh;

        let idx =
            self.stats
                .record_cycle(wire_round_ns, phase_wait_ns, all_fresh, wc_ok, &self.stale);

        let obs = CycleObservation {
            cycle_index: idx,
            wire_round_ns,
            phase_wait_ns,
            all_devices_fresh: all_fresh,
            wc_ok,
            stale_device_count: stale_count,
        };
        self.recorder.on_connector_cycle(&obs);

        if faulted {
            return Err(BusFault);
        }
        Ok(CycleQuality {
            cycle_index: idx,
            all_devices_fresh: all_fresh,
        })
    }

    fn read_input(&self, _r: &usize, _dst: &mut [u8]) -> Validity {
        Validity::Fresh
    }

    fn write_output(&mut self, _r: &usize, _src: &[u8]) {}
}

impl CyclicFieldbusTelemetry<N> for RefBus {
    fn cycle_stats(&self) -> ConnectorCycleSnapshot<N> {
        self.stats.snapshot()
    }
}

#[test]
fn happy_path_folds_and_pushes_each_cycle() {
    let mut bus = RefBus::new();
    for _ in 0..5 {
        pollster::block_on(bus.exchange()).expect("ok cycle");
    }
    let seen = bus.recorder.seen.borrow();
    assert_eq!(seen.len(), 5);
    assert!(seen.iter().all(|o| o.outcome() == CycleOutcome::Completed));
    // cycle_index is dense 0..5.
    assert_eq!(
        seen.iter().map(|o| o.cycle_index).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );

    let snap = bus.cycle_stats();
    assert_eq!(snap.wire_round_min, 1000);
    assert_eq!(snap.phase_wait_min, 500);
    assert_eq!(snap.wc_mismatch_count, 0);
}

#[test]
fn fault_still_emits_advances_index_and_does_not_poison() {
    let mut bus = RefBus::new();

    // Cycle 0: hard fault. Returns Err but must still push + advance index.
    bus.fault_next = true;
    let err = pollster::block_on(bus.exchange());
    assert!(err.is_err());
    {
        let seen = bus.recorder.seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].cycle_index, 0);
        assert_eq!(seen[0].outcome(), CycleOutcome::Fault);
        assert_eq!(seen[0].wire_round_ns, None);
    }
    // Poison-safe: no duration sample recorded yet.
    assert_eq!(bus.cycle_stats().wire_round_min, 0);

    // Cycle 1: good. Index advanced through the fault (0 -> 1).
    pollster::block_on(bus.exchange()).expect("ok cycle");
    {
        let seen = bus.recorder.seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].cycle_index, 1);
    }
    // Only the good cycle contributed a sample.
    assert_eq!(bus.cycle_stats().wire_round_min, 1000);
    // The fault counted as a working-counter mismatch.
    assert_eq!(bus.cycle_stats().wc_mismatch_count, 1);
}

#[test]
fn degraded_cycle_counts_staleness() {
    let mut bus = RefBus::new();
    bus.stale = [true, false];
    pollster::block_on(bus.exchange()).expect("degraded but ok");
    let seen = bus.recorder.seen.borrow();
    assert_eq!(seen[0].outcome(), CycleOutcome::Degraded);
    assert_eq!(seen[0].stale_device_count, 1);
    assert!(!seen[0].all_devices_fresh);

    let snap = bus.cycle_stats();
    assert_eq!(snap.not_all_fresh_count, 1);
    assert_eq!(snap.per_device_max_stale, [1, 0]);
    // A degraded cycle still has a valid wire round (not poison-skipped).
    assert_eq!(snap.wire_round_min, 1000);
}
