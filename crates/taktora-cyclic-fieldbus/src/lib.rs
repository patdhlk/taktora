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

/// A fieldbus that exchanges a coherent process-data image on a fixed
/// cycle (`REQ_0853`). `EtherCAT` and (later) `CANopen` implement it; the
/// event-driven raw-CAN/host/zenoh connectors do not.
///
/// `exchange()` owns cycle timing (`REQ_0852`): it waits for the cycle
/// phase, performs the wire round, and returns a [`CycleQuality`]. The
/// consumer (`taktora-motion`) brackets it with `read_input` /
/// `write_output` and never holds a timer.
#[allow(async_fn_in_trait)] // single-runtime, in-crate consumers only
pub trait CyclicFieldbus {
    /// Addresses one process-data slice within the image.
    type Routing;
    /// Error returned by a failed wire round.
    type Error: core::fmt::Debug;

    /// Wait for the cycle phase, perform the wire round, return quality.
    async fn exchange(&mut self) -> Result<CycleQuality, Self::Error>;

    /// Copy this cycle's input slice for `routing` into `dst`; report the
    /// owning device's [`Validity`].
    fn read_input(&self, routing: &Self::Routing, dst: &mut [u8]) -> Validity;

    /// Stage `src` into the output image at `routing` for the next
    /// `exchange()`.
    fn write_output(&mut self, routing: &Self::Routing, src: &[u8]);
}

#[cfg(test)]
mod trait_tests {
    use super::*;

    struct OneByteBus {
        out: u8,
        inp: u8,
        cycle: u64,
    }

    impl CyclicFieldbus for OneByteBus {
        type Routing = (); // single device, single byte each direction
        type Error = core::convert::Infallible;

        async fn exchange(&mut self) -> Result<CycleQuality, Self::Error> {
            self.inp = self.out.wrapping_add(1); // virtual device echoes out+1
            let q = CycleQuality {
                cycle_index: self.cycle,
                all_devices_fresh: true,
            };
            self.cycle += 1;
            Ok(q)
        }
        fn read_input(&self, _r: &(), dst: &mut [u8]) -> Validity {
            dst[0] = self.inp;
            Validity::Fresh
        }
        fn write_output(&mut self, _r: &(), src: &[u8]) {
            self.out = src[0];
        }
    }

    #[test]
    fn drives_a_virtual_device_through_the_trait() {
        let mut bus = OneByteBus {
            out: 0,
            inp: 0,
            cycle: 0,
        };
        bus.write_output(&(), &[41]);
        let q = block_on(bus.exchange()).unwrap();
        assert_eq!(q.cycle_index, 0);
        let mut got = [0u8; 1];
        assert_eq!(bus.read_input(&(), &mut got), Validity::Fresh);
        assert_eq!(got[0], 42);
    }

    // tiny ad-hoc executor to avoid a dev-dep in this leaf crate
    #[allow(unsafe_code)]
    fn block_on<F: core::future::Future>(mut f: F) -> F::Output {
        use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(core::ptr::null(), &VT)
        }
        static VT: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        // SAFETY:
        //   1. Waker: the vtable's clone/wake/drop are all no-ops over a null
        //      data pointer, so `Waker::from_raw` is trivially valid and the
        //      pointer is never dereferenced.
        //   2. Pin: `f` is a local owned by this function and borrowed for the
        //      rest of its body; it is never moved before the pinned reference
        //      is dropped, satisfying `Pin::new_unchecked`'s contract.
        let w = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VT)) };
        let mut cx = Context::from_waker(&w);
        let mut f = unsafe { core::pin::Pin::new_unchecked(&mut f) };
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }
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
