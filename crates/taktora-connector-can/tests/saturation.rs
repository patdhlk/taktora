//! Inbound-saturation coverage for the CAN connector (`REQ_0608`).
//!
//! The bridge-unit contract lives next to the type (see
//! `bridge::tests::inbound_drop_count_monotonically_increases`); these
//! tests pin the integration with [`CanHealthMonitor`] —
//! [`BridgedInboundPublish`] feeds [`InboundOutcome::Dropped`]'s
//! cumulative count into
//! [`CanHealthMonitor::record_inbound_drop`], which emits a single
//! `ConnectorHealth::Degraded { reason: "dropped N inbound frames" }`
//! transition once the configured threshold is crossed.

use std::sync::Arc;

use taktora_connector_can::{
    BridgedInboundPublish, CanHealthMonitor, CanIface, IfaceHealthKind, InboundPublish,
};
use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};

fn iface(name: &str) -> CanIface {
    CanIface::new(name).unwrap()
}

/// `REQ_0608`: a small-capacity inbound bridge driven past its bound
/// records the drop count and emits a single `Degraded` transition once
/// the cumulative drop count crosses the connector's
/// `inbound_drop_threshold`.
#[test]
fn inbound_overflow_emits_degraded_transition() {
    let a = iface("vcan0");
    let health = Arc::new(CanHealthMonitor::new(&[a]));
    let _ = health.set_iface(a, IfaceHealthKind::Up).unwrap();
    let sub = health.subscribe();
    // Drain the bring-up Connecting → Up event so the test asserts
    // only on the drops-driven transition that follows.
    while sub.try_recv().is_ok() {}

    // Capacity 1, threshold 2 — the first publish fills the slot,
    // self-drains, and is forwarded; the second through fourth land
    // in the bridge faster than the drain pattern can absorb them
    // because we deliberately swap the drain order in
    // `BridgedInboundPublish::without_transport`'s test path: the
    // test path SKIPS the synchronous drain (no iceoryx2 transport
    // to forward to), so the bridge fills on the second send.
    //
    // Construction details: a 0-payload publish carries no SHM cost,
    // so we exercise just the bridge accounting.
    let publish: BridgedInboundPublish<8> =
        BridgedInboundPublish::without_transport(1, Arc::clone(&health), 2);

    // First publish: bridge.try_send succeeds (Sent), test stub drains
    // immediately, no drop.
    publish.publish_bytes(b"frame1").expect("first publish");
    // Second publish: same — drains.
    publish.publish_bytes(b"frame2").expect("second publish");
    // Third / fourth / fifth publishes from concurrent threads would
    // normally fill the bridge. In the single-threaded test we
    // simulate that by sending directly into the bridge below.

    // Force the bridge into overflow by manually filling it.
    let bridge = publish.bridge();
    // Capacity is 1; one Sent fills it.
    assert!(matches!(
        bridge.try_send(()),
        taktora_connector_can::InboundOutcome::Sent
    ));
    // Now overflow — every subsequent try_send reports Dropped.
    let _ = bridge.try_send(());
    let _ = bridge.try_send(());
    // Drive one more publish through the wrapper; the bridge is
    // already full, so we expect Dropped and a Degraded transition.
    publish
        .publish_bytes(b"frame3")
        .expect("publish reports Ok");

    assert!(
        health.degraded_due_to_drops(),
        "drops-latch should be set after threshold crossed"
    );
    assert_eq!(health.current().kind(), ConnectorHealthKind::Degraded);

    // The Up → Degraded event hit the subscribe channel.
    let evt = sub
        .try_recv()
        .expect("Degraded transition broadcast to subscribers");
    assert_eq!(evt.from.kind(), ConnectorHealthKind::Up);
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
    match &evt.to {
        ConnectorHealth::Degraded { reason } => {
            assert!(
                reason.contains("dropped"),
                "reason {reason:?} must mention dropped count"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

/// `REQ_0608` recovery: once the gateway transitions back to `Up` via
/// the iface aggregator, the drops-latch re-arms so a fresh burst can
/// re-emit `Degraded`.
#[test]
fn recovery_to_up_rearms_drops_latch() {
    let a = iface("vcan0");
    let health = Arc::new(CanHealthMonitor::new(&[a]));
    let _ = health.set_iface(a, IfaceHealthKind::Up).unwrap();

    // Force a Degraded emission via the drops path.
    let evt = health
        .record_inbound_drop(5, 1)
        .expect("crossing threshold emits Degraded");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
    assert!(health.degraded_due_to_drops());

    // Recovery — re-setting iface to Up aggregates to Up; the legal
    // `Degraded → Up` edge fires and clears the drops-latch.
    let _ = health.set_iface(a, IfaceHealthKind::Up).unwrap();
    assert!(!health.degraded_due_to_drops());
    assert_eq!(health.current().kind(), ConnectorHealthKind::Up);

    // A fresh burst now re-emits Degraded.
    let evt2 = health
        .record_inbound_drop(7, 1)
        .expect("second crossing re-emits Degraded");
    assert_eq!(evt2.to.kind(), ConnectorHealthKind::Degraded);
}
