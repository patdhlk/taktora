//! SubDevice sync-manager watchdog register model. `REQ_0846`.
//!
//! EtherCAT slaves carry a process-data (sync-manager) watchdog whose
//! window is configured by two ESC registers: `0x0400` (watchdog
//! divider — the tick base) and `0x0420` (SM watchdog time, in ticks).
//! Per ETG.1000.4 a tick is `40 ns × (divider + 2)`; the ESC powers up
//! with divider `2498` (a 100 µs tick) and `1000` ticks, i.e. a 100 ms
//! window. That default **violates** safety assumption `AOU_0016`,
//! which requires an output slave's watchdog timeout ≤ FTTI/2 = 50 ms,
//! and ESI files carry no timeout data — so the master must program
//! these registers itself. [`SmWatchdog`] is the pure value the
//! gateway writes; the register I/O lives in
//! [`crate::EthercrabBusDriver`] and is hardware-verified. Field names
//! mirror IgH's `ecrt_slave_config_watchdog(divider, intervals)`.

/// Default watchdog divider (`0x0400`) — a 100 µs tick.
///
/// `40 ns × (2498 + 2) = 100 µs`, matching the ESC power-up value. We
/// fix this and vary only the tick count, so a timeout quantizes to a
/// whole number of 100 µs ticks.
pub const DEFAULT_DIVIDER: u16 = 2498;

/// SubDevice SM-watchdog register values: divider (`0x0400`) and tick
/// count (`0x0420`).
///
/// A pure descriptor — construct it via [`SmWatchdog::from_timeout_us`]
/// (or directly when the raw register values are known) and hand it to
/// the driver, which writes both registers and reads them back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmWatchdog {
    /// Watchdog divider register `0x0400`. Tick = `40 ns × (divider + 2)`.
    pub divider: u16,
    /// SM watchdog time register `0x0420`, in ticks.
    pub intervals: u16,
}

impl SmWatchdog {
    /// Build a watchdog from a timeout in microseconds, fixing the
    /// divider at [`DEFAULT_DIVIDER`] (100 µs ticks).
    ///
    /// `intervals = ceil(timeout_us / 100)`, clamped to `1..=u16::MAX`.
    /// Quantization is upward: a request that is not a whole multiple of
    /// 100 µs rounds **up** to the next tick, so the effective timeout
    /// is `≥ timeout_us` — callers checking an upper bound (e.g. the
    /// AOU_0016 FTTI/2 ceiling) must test [`Self::effective_timeout_ns`],
    /// not the request. `0 µs` clamps to one tick rather than the
    /// disabling 0-interval value: this helper never emits a disabled
    /// watchdog.
    #[must_use]
    pub const fn from_timeout_us(timeout_us: u32) -> Self {
        // ceil(timeout_us / 100), then clamp to 1..=u16::MAX before the
        // narrowing conversion. Clamping in u32 keeps the `as u16` lossless.
        let ticks = timeout_us.div_ceil(100);
        let intervals = if ticks < 1 {
            1
        } else if ticks > u16::MAX as u32 {
            u16::MAX
        } else {
            // Provably lossless: this branch only runs when
            // `ticks <= u16::MAX`. `try_from` is not const, so cast.
            #[allow(clippy::cast_possible_truncation)]
            {
                ticks as u16
            }
        };
        Self {
            divider: DEFAULT_DIVIDER,
            intervals,
        }
    }

    /// Effective watchdog window in nanoseconds: the actual timeout the
    /// hardware enforces given these register values.
    ///
    /// `40 ns × (divider + 2) × intervals`. Computed in `u64`; the
    /// worst case (`2500 × 65535 × 40`) is far inside `u64`.
    #[must_use]
    pub const fn effective_timeout_ns(&self) -> u64 {
        40 * (self.divider as u64 + 2) * self.intervals as u64
    }
}
