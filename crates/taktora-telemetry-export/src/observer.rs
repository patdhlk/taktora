//! [`NdjsonRingObserver`] — the producer side of telemetry export.

use taktora_executor::{CycleObservation, Observer};

use crate::record::PodRecord;
use crate::ring::Producer;

/// An [`Observer`] that flattens each cycle push into a [`PodRecord`] and
/// pushes it into the export ring.
///
/// Does no I/O: the drain thread (see [`crate::writer`]) turns records into
/// NDJSON off the RT thread.
///
/// Drive this from a single executor (the contract `Producer` relies on).
pub struct NdjsonRingObserver {
    producer: Producer,
}

impl NdjsonRingObserver {
    /// Wrap the producer half of a [`crate::CycleRing`].
    #[must_use]
    pub const fn new(producer: Producer) -> Self {
        Self { producer }
    }
}

impl Observer for NdjsonRingObserver {
    fn on_cycle_stats(&self, obs: &CycleObservation) {
        self.producer.push(PodRecord::from_observation(obs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::{CycleRing, RecvOutcome};

    fn obs(cycle_index: u64, task_index: u32) -> CycleObservation {
        CycleObservation {
            cycle_index,
            task_id: taktora_executor::TaskId::from("t"),
            task_index,
            faulted: false,
            period_ns: 1_000_000,
            pre_ns: 42,
            actual_period_ns: Some(1_000_010),
            jitter_ns: Some(10),
            lateness_ns: Some(-3),
            took_ns: Some(250),
            skipped_slots: 0,
        }
    }

    #[test]
    fn pushes_flattened_record_per_cycle() {
        let (producer, mut consumer) = CycleRing::with_capacity(8).split();
        let observer = NdjsonRingObserver::new(producer);
        observer.on_cycle_stats(&obs(0, 1));
        observer.on_cycle_stats(&obs(1, 1));

        match consumer.try_recv() {
            RecvOutcome::Record(r) => {
                assert_eq!(r.cycle_index, 0);
                assert_eq!(r.task_index, 1);
                assert_eq!(r.ts_ns, 42, "ts_ns comes from pre_ns");
                assert_eq!(r.took_ns, 250);
            }
            other => panic!("expected Record, got {other:?}"),
        }
        match consumer.try_recv() {
            RecvOutcome::Record(r) => assert_eq!(r.cycle_index, 1),
            other => panic!("expected Record, got {other:?}"),
        }
    }
}
