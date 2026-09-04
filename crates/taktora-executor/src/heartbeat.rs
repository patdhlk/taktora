//! Heartbeat tick emitted by the executor to signal liveness.

/// A single heartbeat tick emitted by the executor's dispatch loop.
///
/// Carries a monotonically increasing sequence number and a timestamp
/// sampled from the executor's cyclic clock. Emitted at a bounded period
/// (configured via [`crate::ExecutorBuilder::heartbeat`]) for liveness
/// monitoring. Alloc-free on the hot path.
///
/// # Timing contract
///
/// When configured, the executor guarantees a tick is emitted at least
/// every `period` (subject to OS scheduler latency). The inter-tick gap
/// is bounded by approximately `period * 2` under typical load. An
/// external monitor can force a safe state if no tick arrives within
/// its configured timeout (typically FTTI/2, ≤ 50 ms for automotive).
///
/// # TSR coverage
///
/// Supports `TSR_0010` / `AOU_0003`: the executor emits liveness
/// evidence on a bounded schedule, surfaced via
/// [`crate::Observer::on_heartbeat`] and bridged by
/// `taktora-connector-host` onto a `HealthEvent` channel for external
/// watchdog integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeartbeatTick {
    /// Monotonically increasing sequence number.
    ///
    /// Increments by 1 per tick. Starts at 1 on the first emitted tick.
    /// Wraps at `u64::MAX` (unreachable in practice: 584 million years
    /// at 1 ms period).
    pub seq: u64,

    /// Timestamp in nanoseconds since the cyclic clock's epoch.
    ///
    /// Sampled from the executor's scheduling clock
    /// ([`crate::CyclicClock::now_nanos`]) at the moment the tick is
    /// emitted. Monotonic within one executor instance.
    pub at_nanos: u64,
}
