//! Inbound-saturation coverage for the `EtherCAT` connector (`REQ_0324`).
//!
//! The bridge-unit contract lives next to the type (see
//! `tests/bridge.rs` for the bounded `InboundBridge` semantics); these
//! tests pin the integration with [`EthercatHealthMonitor`] —
//! [`BridgedInboundPublish`] feeds [`InboundOutcome::Dropped`]'s
//! cumulative count into
//! [`EthercatHealthMonitor::record_inbound_drop`], which emits a
//! single `ConnectorHealth::Degraded { reason: "dropped N inbound frames" }`
//! transition once the configured threshold is crossed.

use std::sync::Arc;

use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};
use taktora_connector_ethercat::{BridgedInboundPublish, EthercatHealthMonitor, InboundPublish};

/// `REQ_0324`: a small-capacity inbound bridge driven past its bound
/// records the drop count and emits a single `Degraded` transition
/// once the cumulative drop count crosses the connector's
/// `inbound_drop_threshold`.
#[test]
fn inbound_overflow_emits_degraded_transition() {
    let health = Arc::new(EthercatHealthMonitor::new());
    // Bring the monitor up from the initial Connecting state so the
    // drops-driven Up → Degraded edge has a legal source.
    health.transition_to(ConnectorHealth::Up).unwrap();
    let sub = health.subscribe();
    while sub.try_recv().is_ok() {}

    // Capacity 1, threshold 2 — drive 3+ inbound PDUs into the bridge
    // to trigger overflow.
    let publish: BridgedInboundPublish<8> =
        BridgedInboundPublish::without_transport(1, Arc::clone(&health), 2);

    // Manually fill + overflow the bridge.
    let bridge = publish.bridge();
    let _ = bridge.try_send(()); // fills capacity 1
    let _ = bridge.try_send(()); // drop #1
    let _ = bridge.try_send(()); // drop #2
    // Now the wrapper's try_send returns Dropped { count: 3 }, which
    // crosses threshold 2 and emits Degraded.
    publish.publish_bytes(b"pdu").expect("publish returns Ok");

    assert!(health.degraded_due_to_drops());
    assert_eq!(health.current().kind(), ConnectorHealthKind::Degraded);

    let evt = sub
        .try_recv()
        .expect("Degraded transition broadcast to subscribers");
    assert_eq!(evt.from.kind(), ConnectorHealthKind::Up);
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
    match &evt.to {
        ConnectorHealth::Degraded { reason } => {
            assert!(
                reason.contains("dropped"),
                "reason {reason:?} must mention dropped"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

/// `REQ_0324` recovery: when the stack recovers and the runner
/// transitions back to `Up` (e.g. WKC verdict matches again), the
/// drops-latch re-arms so a fresh burst can re-emit `Degraded`.
#[test]
fn recovery_to_up_rearms_drops_latch() {
    let health = Arc::new(EthercatHealthMonitor::new());
    health.transition_to(ConnectorHealth::Up).unwrap();

    let evt = health
        .record_inbound_drop(5, 1)
        .expect("crossing threshold emits Degraded");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
    assert!(health.degraded_due_to_drops());

    // Recovery — Degraded → Up is a legal edge per ARCH_0012; the
    // runner takes this when the WKC verdict goes from mismatch back
    // to match.
    health
        .transition_to(ConnectorHealth::Up)
        .expect("Degraded → Up is legal");
    assert!(!health.degraded_due_to_drops());
    assert_eq!(health.current().kind(), ConnectorHealthKind::Up);

    // A fresh burst now re-emits Degraded.
    let evt2 = health
        .record_inbound_drop(7, 1)
        .expect("second crossing re-emits Degraded");
    assert_eq!(evt2.to.kind(), ConnectorHealthKind::Degraded);
}
