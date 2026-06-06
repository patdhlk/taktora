//! SAFE-OP → OP transition pacing. `REQ_0841`.
//!
//! Some couplers refuse the SAFE-OP → OP transition until they see
//! cyclic output process data: their sync-manager watchdog runs from
//! the moment OP is requested, trips during a traffic-less wait, and
//! the resulting AL error (`0x001B` Sync manager watchdog) **latches**
//! until the master acknowledges it. The WAGO 750-354 is the canonical
//! case — its ESI declares `SafeopOpTimeout=100` (ms), matching the
//! default SM watchdog window. A master that requests OP and then
//! merely polls the AL state (ethercrab's blocking `into_op`)
//! deadlocks: the coupler waits for data, the master waits for OP.
//!
//! The gateway therefore requests OP without waiting
//! (`request_into_op`) and keeps exchanging process data while the
//! group converges. This module is the pure decision core of that
//! wait loop: given the number of completed spins (one spin = one
//! `tx_rx` exchange that did **not** yet observe all SubDevices in
//! OP), decide whether to keep cycling, sweep latched AL errors, or
//! give up. Keeping it pure lets the pacing be unit-tested
//! (`TEST_0857`) without bus hardware; the loop itself lives in
//! [`crate::EthercrabBusDriver`] and is hardware-verified.

/// Spins between latched-AL-error acknowledge sweeps.
///
/// At the loop's ~2 ms spin interval this is roughly one sweep per
/// 200 ms — fast enough to clear a latched watchdog error well inside
/// the bring-up window, slow enough that the sweep's register reads
/// don't crowd out process-data traffic.
pub const OP_WAIT_ACK_INTERVAL: u32 = 100;

/// Spin bound after which the wait gives up.
///
/// With the ~2 ms spin interval (plus per-spin `tx_rx` latency) this
/// is a ≥10 s window — far beyond any sane `SafeopOpTimeout`, so
/// reaching it means the bus genuinely cannot converge to OP.
pub const OP_WAIT_MAX_SPINS: u32 = 5_000;

/// Sleep between spins. Together with the per-spin `tx_rx` latency
/// this paces the wait at roughly the gateway's default cycle time.
pub const OP_WAIT_SPIN_INTERVAL: core::time::Duration = core::time::Duration::from_millis(2);

/// One step of the OP wait loop. See [`op_wait_action`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpWaitAction {
    /// Keep exchanging process data.
    Continue,
    /// Sweep the group for latched AL errors and acknowledge them
    /// (AL Control: OP request + error-acknowledge), then keep
    /// exchanging.
    AckLatchedErrors,
    /// The bound is exceeded — fail the bring-up / recovery attempt.
    GiveUp,
}

/// Pure decision: what should the OP wait loop do after `spins`
/// completed exchanges that did not yet observe the whole group in
/// OP? Spin counts start at 1.
#[must_use]
pub const fn op_wait_action(spins: u32) -> OpWaitAction {
    if spins > OP_WAIT_MAX_SPINS {
        OpWaitAction::GiveUp
    } else if spins % OP_WAIT_ACK_INTERVAL == 0 {
        OpWaitAction::AckLatchedErrors
    } else {
        OpWaitAction::Continue
    }
}
