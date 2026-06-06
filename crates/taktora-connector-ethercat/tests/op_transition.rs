//! TEST_0857 — SAFE-OP → OP wait-loop pacing (`REQ_0841`). Pure
//! logic; the cyclic `tx_rx` exchange itself is hardware-verified via
//! `tests/ethercrab_driver.rs` against a real bus.

#![allow(clippy::doc_markdown)]

use taktora_connector_ethercat::op_transition::{
    OP_WAIT_ACK_INTERVAL, OP_WAIT_MAX_SPINS, OpWaitAction, op_wait_action,
};

#[test]
fn first_spin_continues() {
    assert_eq!(op_wait_action(1), OpWaitAction::Continue);
}

#[test]
fn spins_between_ack_sweeps_continue() {
    assert_eq!(
        op_wait_action(OP_WAIT_ACK_INTERVAL - 1),
        OpWaitAction::Continue
    );
    assert_eq!(
        op_wait_action(OP_WAIT_ACK_INTERVAL + 1),
        OpWaitAction::Continue
    );
}

#[test]
fn every_ack_interval_acknowledges_latched_errors() {
    assert_eq!(
        op_wait_action(OP_WAIT_ACK_INTERVAL),
        OpWaitAction::AckLatchedErrors
    );
    assert_eq!(
        op_wait_action(OP_WAIT_ACK_INTERVAL * 3),
        OpWaitAction::AckLatchedErrors
    );
}

#[test]
fn final_in_bound_spin_still_acknowledges() {
    // OP_WAIT_MAX_SPINS is itself a multiple of the ack interval; the
    // bound is exceeded only strictly beyond it.
    assert_eq!(
        op_wait_action(OP_WAIT_MAX_SPINS),
        OpWaitAction::AckLatchedErrors
    );
}

#[test]
fn beyond_bound_gives_up() {
    assert_eq!(op_wait_action(OP_WAIT_MAX_SPINS + 1), OpWaitAction::GiveUp);
    assert_eq!(op_wait_action(u32::MAX), OpWaitAction::GiveUp);
}
