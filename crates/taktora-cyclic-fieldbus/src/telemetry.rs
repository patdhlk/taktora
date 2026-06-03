//! Connector telemetry seam (`REQ_0265`): the push observer hook and the
//! pull snapshot accessor, mirroring the executor's two paths (`REQ_0103`).

use taktora_stats::ConnectorCycleSnapshot;

use crate::CycleObservation;

/// Push path (`REQ_0265`): consumer-implemented observer, called per cycle.
///
/// The connector invokes this once per `exchange()` call, on every path
/// including error (`REQ_0267`). The method has a no-op default so a consumer
/// that only wants the pull path need not implement it — mirroring the
/// executor's `Observer` (`REQ_0103`).
pub trait ConnectorCycleObserver {
    /// Called once per `exchange()` with the raw per-cycle observation.
    fn on_connector_cycle(&self, _obs: &CycleObservation) {}
}

/// No-op observer for connectors used without a push consumer.
pub struct NoopConnectorObserver;
impl ConnectorCycleObserver for NoopConnectorObserver {}

/// Pull path (`REQ_0265`): snapshot accessor for per-bus cycle aggregates.
///
/// A cyclic connector that collects cycle telemetry exposes a borrowed-free
/// snapshot of its current per-bus aggregates, readable concurrently with
/// the cyclic exchange via relaxed-atomic reads. `N` is the device count.
pub trait CyclicFieldbusTelemetry<const N: usize> {
    /// Current per-bus aggregate snapshot.
    fn cycle_stats(&self) -> ConnectorCycleSnapshot<N>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CycleObservation;
    use core::cell::Cell;
    use taktora_stats::{ConnectorCycleSnapshot, ConnectorCycleStats};

    #[test]
    fn noop_observer_default_is_callable() {
        let obs = CycleObservation {
            cycle_index: 0,
            wire_round_ns: Some(1),
            phase_wait_ns: Some(1),
            all_devices_fresh: true,
            wc_ok: true,
            stale_device_count: 0,
        };
        // Compiles and runs: the default method is a no-op.
        NoopConnectorObserver.on_connector_cycle(&obs);
    }

    #[test]
    fn custom_observer_receives_the_observation() {
        struct Counter {
            last_index: Cell<u64>,
            calls: Cell<u32>,
        }
        impl ConnectorCycleObserver for Counter {
            fn on_connector_cycle(&self, obs: &CycleObservation) {
                self.last_index.set(obs.cycle_index);
                self.calls.set(self.calls.get() + 1);
            }
        }
        let c = Counter {
            last_index: Cell::new(0),
            calls: Cell::new(0),
        };
        let obs = CycleObservation {
            cycle_index: 42,
            wire_round_ns: None,
            phase_wait_ns: None,
            all_devices_fresh: false,
            wc_ok: false,
            stale_device_count: 1,
        };
        c.on_connector_cycle(&obs);
        assert_eq!(c.calls.get(), 1);
        assert_eq!(c.last_index.get(), 42);
    }

    #[test]
    fn telemetry_trait_returns_a_snapshot() {
        struct Bus;
        impl CyclicFieldbusTelemetry<2> for Bus {
            fn cycle_stats(&self) -> ConnectorCycleSnapshot<2> {
                ConnectorCycleStats::<2, 4, 64>::new(100).snapshot()
            }
        }
        let snap = Bus.cycle_stats();
        assert_eq!(snap.wire_round_min, 0);
        assert_eq!(snap.per_device_max_stale, [0, 0]);
    }
}
