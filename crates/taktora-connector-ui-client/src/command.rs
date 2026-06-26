//! Command invocation policy: correlation-id minting and the epoch-aware retry
//! decision (REQ_0865, REQ_0867, REQ_0868, REQ_0882).
//!
//! The pure, unit-testable heart of command invocation lives here; the iceoryx2
//! request/reply round-trip that drives it lives in [`crate::client`].
//!
//! # Correlation ids (REQ_0867)
//!
//! Every invocation carries a unique 32-byte correlation id that the server uses
//! both as its dedupe key and as the reply-routing key. A retry **reuses** the
//! same id so the server can replay its cached acceptance ack instead of
//! re-running the effect (at-most-once delivery). The client is responsible for
//! minting globally-unique ids across commands — [`mint_correlation_id`] does so
//! from the process id, a startup timestamp, and a monotonic counter.
//!
//! # Retry policy (REQ_0868, REQ_0882)
//!
//! On a client-side timeout (no ack within the bound), whether to retry depends
//! on the command's `idempotent` flag and whether the connector's **epoch** has
//! changed (a restart, REQ_0882):
//!
//! * An **idempotent** command auto-retries (reusing the id) up to a max-attempt
//!   bound, *including across an epoch change* — re-running it is harmless.
//! * A **non-idempotent** command auto-retries only **within the same epoch**
//!   (where the server's correlation-id dedupe still guarantees at-most-once); it
//!   does **not** retry across an epoch boundary. An in-flight non-idempotent
//!   command whose epoch changed surfaces as [`RetryDecision::OutcomeUnknown`]
//!   for operator resolution, because the restarted server lost the dedupe state
//!   that made a resend safe.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use taktora_connector_transport_iox::envelope::CorrelationId;

/// What to do after a command invocation timed out waiting for its ack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryDecision {
    /// Resend the request under the **same** correlation id and keep waiting.
    Retry,
    /// Stop retrying: the max-attempt bound is exhausted (surface a timeout).
    GiveUp,
    /// Stop retrying: a non-idempotent command crossed an epoch boundary, so the
    /// outcome cannot be known — surface it for operator resolution (REQ_0882).
    OutcomeUnknown,
}

/// Decide whether to retry a timed-out invocation (REQ_0868, REQ_0882).
///
/// Pure and total over its inputs:
/// * `idempotent` — the command's manifest flag.
/// * `epoch_changed` — whether the connector epoch changed since the invocation
///   began (a server restart).
/// * `attempts` — how many sends have already been made (≥ 1 after the first).
/// * `max_attempts` — the client's attempt bound (clamped to ≥ 1).
///
/// Rules:
/// * non-idempotent **and** `epoch_changed` → [`RetryDecision::OutcomeUnknown`]
///   (never resend across a restart for a non-idempotent command);
/// * otherwise retry while `attempts < max_attempts`, else
///   [`RetryDecision::GiveUp`].
///
/// Idempotent commands retry across an epoch change; non-idempotent commands
/// retry only within the same epoch (where server-side correlation-id dedupe
/// preserves at-most-once).
#[must_use]
pub fn retry_decision(
    idempotent: bool,
    epoch_changed: bool,
    attempts: u32,
    max_attempts: u32,
) -> RetryDecision {
    if epoch_changed && !idempotent {
        return RetryDecision::OutcomeUnknown;
    }
    let max = max_attempts.max(1);
    if attempts < max {
        RetryDecision::Retry
    } else {
        RetryDecision::GiveUp
    }
}

/// Mint a globally-unique 32-byte [`CorrelationId`] for one invocation
/// (REQ_0867).
///
/// Layout: bytes `0..4` = process id, `4..12` = process-startup wall-clock nanos
/// (stable within the process), `12..20` = a monotonically increasing per-process
/// counter; the rest is zero. The startup term separates two processes and the
/// counter separates invocations within a process, so two `mint`s never collide
/// (across commands too — the server keys its dedupe on the bare id).
#[must_use]
pub fn mint_correlation_id() -> CorrelationId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let pid = std::process::id();
    let startup = startup_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut id = [0u8; 32];
    id[0..4].copy_from_slice(&pid.to_le_bytes());
    id[4..12].copy_from_slice(&startup.to_le_bytes());
    id[12..20].copy_from_slice(&seq.to_le_bytes());
    id
}

/// The process-startup wall-clock nanosecond reading, cached so it is stable for
/// the process lifetime. Used as the per-process entropy term in a correlation
/// id so ids minted by distinct client incarnations do not collide.
fn startup_nanos() -> u64 {
    use std::sync::OnceLock;
    static STARTUP: OnceLock<u64> = OnceLock::new();
    *STARTUP.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotent_retries_within_attempt_bound() {
        assert_eq!(
            retry_decision(true, false, 1, 3),
            RetryDecision::Retry,
            "first timeout (1 attempt) of 3 retries"
        );
        assert_eq!(retry_decision(true, false, 2, 3), RetryDecision::Retry);
        assert_eq!(
            retry_decision(true, false, 3, 3),
            RetryDecision::GiveUp,
            "attempts exhausted"
        );
    }

    #[test]
    fn idempotent_retries_across_epoch_change() {
        // A restart is harmless for an idempotent command: keep retrying.
        assert_eq!(retry_decision(true, true, 1, 3), RetryDecision::Retry);
    }

    #[test]
    fn non_idempotent_retries_within_same_epoch() {
        // Same epoch: server dedupe makes a resend at-most-once, so retry.
        assert_eq!(retry_decision(false, false, 1, 3), RetryDecision::Retry);
        assert_eq!(retry_decision(false, false, 3, 3), RetryDecision::GiveUp);
    }

    #[test]
    fn non_idempotent_across_epoch_is_outcome_unknown() {
        // A restart wiped the dedupe state -> a resend could double-execute.
        assert_eq!(
            retry_decision(false, true, 1, 3),
            RetryDecision::OutcomeUnknown
        );
        // Even with attempts left, the epoch boundary short-circuits.
        assert_eq!(
            retry_decision(false, true, 1, 99),
            RetryDecision::OutcomeUnknown
        );
    }

    #[test]
    fn max_attempts_is_clamped_to_at_least_one() {
        // A zero bound must still allow the decision to terminate (no panic, no
        // infinite retry): treat it as one attempt -> give up immediately.
        assert_eq!(retry_decision(true, false, 1, 0), RetryDecision::GiveUp);
    }

    #[test]
    fn minted_ids_are_unique() {
        let a = mint_correlation_id();
        let b = mint_correlation_id();
        assert_ne!(a, b, "two mints must differ");
        assert_eq!(a.len(), 32);
    }
}
