//! Connector-side telemetry value types (`REQ_0265`, `REQ_0267`): the
//! per-cycle push observation and its three-way outcome classification.

/// Coarse classification of one `exchange()` cycle (`REQ_0267`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleOutcome {
    /// Wire round completed and every device was fresh.
    Completed,
    /// Wire round completed but a device was stale or the working counter
    /// mismatched — drives the `Degraded` connector state (`REQ_0230`).
    Degraded,
    /// The wire round errored; no valid duration this cycle.
    Fault,
}

/// Raw per-cycle observation pushed once per `exchange()` call on every
/// path, including error (`REQ_0265`, `REQ_0267`). Durations are
/// nanoseconds and `None` on a cycle that produced no valid value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleObservation {
    /// Monotonic cycle counter (zero-indexed, increments on every exchange
    /// attempt including faults). See `REQ_0107` for the cross-layer
    /// equality invariant with the executor scan count.
    pub cycle_index: u64,
    /// Wire-round duration (ns); `None` on a hard fault.
    pub wire_round_ns: Option<u32>,
    /// Cycle-phase wait / slack (ns); `None` if not measured this cycle.
    pub phase_wait_ns: Option<u32>,
    /// Every participating device was fresh this cycle.
    pub all_devices_fresh: bool,
    /// Working counter (or protocol-equivalent participation check) matched.
    pub wc_ok: bool,
    /// Number of devices stale this cycle.
    pub stale_device_count: u16,
}

impl CycleObservation {
    /// Classify this cycle's outcome (`REQ_0267`). `Fault` whenever the
    /// wire round produced no duration; otherwise `Completed` if fully
    /// fresh and working-counter-OK, else `Degraded`.
    #[must_use]
    pub const fn outcome(&self) -> CycleOutcome {
        if self.wire_round_ns.is_none() {
            CycleOutcome::Fault
        } else if self.all_devices_fresh && self.wc_ok {
            CycleOutcome::Completed
        } else {
            CycleOutcome::Degraded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_classified_when_wire_round_absent() {
        let obs = CycleObservation {
            cycle_index: 7,
            wire_round_ns: None,
            phase_wait_ns: Some(600),
            all_devices_fresh: false,
            wc_ok: false,
            stale_device_count: 2,
        };
        assert_eq!(obs.outcome(), CycleOutcome::Fault);
    }

    #[test]
    fn completed_classified_when_fresh_and_wc_ok() {
        let obs = CycleObservation {
            cycle_index: 0,
            wire_round_ns: Some(1000),
            phase_wait_ns: Some(500),
            all_devices_fresh: true,
            wc_ok: true,
            stale_device_count: 0,
        };
        assert_eq!(obs.outcome(), CycleOutcome::Completed);
    }

    #[test]
    fn degraded_classified_when_round_present_but_not_fresh() {
        let obs = CycleObservation {
            cycle_index: 1,
            wire_round_ns: Some(1000),
            phase_wait_ns: Some(500),
            all_devices_fresh: false,
            wc_ok: true,
            stale_device_count: 1,
        };
        assert_eq!(obs.outcome(), CycleOutcome::Degraded);
    }

    #[test]
    fn degraded_classified_when_round_present_but_wc_mismatch() {
        let obs = CycleObservation {
            cycle_index: 2,
            wire_round_ns: Some(1000),
            phase_wait_ns: Some(500),
            all_devices_fresh: true,
            wc_ok: false,
            stale_device_count: 0,
        };
        assert_eq!(obs.outcome(), CycleOutcome::Degraded);
    }
}
