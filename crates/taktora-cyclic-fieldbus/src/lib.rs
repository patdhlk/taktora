//! See plan docs/superpowers/plans/2026-06-02-motion-nc-spine.md
#![warn(missing_docs)]

/// Per-device freshness of an input slice this cycle (`REQ_0853`).
///
/// Keyed per device (`EtherCAT` `SubDevice` / `CANopen` node) — the analogue
/// of `TwinCAT` `WcState`. A slice's freshness is its owning device's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    /// The owning device participated in this cycle's exchange.
    Fresh,
    /// The device has not participated for `cycles` consecutive cycles.
    Stale {
        /// Consecutive cycles the device has been absent.
        cycles: u32,
    },
    /// The device has never produced valid input.
    NeverSeen,
}

impl Validity {
    /// `true` only for `Fresh`.
    #[must_use]
    pub const fn is_fresh(&self) -> bool {
        matches!(self, Self::Fresh)
    }
}

/// Fieldbus-neutral summary of one completed cycle (`REQ_0853`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleQuality {
    /// Monotonic cycle counter, zero-indexed.
    pub cycle_index: u64,
    /// `true` when every participating device was `Fresh` this cycle.
    pub all_devices_fresh: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_freshness() {
        assert!(Validity::Fresh.is_fresh());
        assert!(!Validity::Stale { cycles: 3 }.is_fresh());
        assert!(!Validity::NeverSeen.is_fresh());
    }

    #[test]
    fn cycle_quality_reports_all_fresh() {
        let q = CycleQuality {
            cycle_index: 7,
            all_devices_fresh: true,
        };
        assert_eq!(q.cycle_index, 7);
        assert!(q.all_devices_fresh);
    }
}
