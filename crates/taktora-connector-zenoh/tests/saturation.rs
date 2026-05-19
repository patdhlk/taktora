//! Saturation tests covering `REQ_0405` (`BackPressure` on outbound),
//! `REQ_0406` (inbound subscriber saturation), and `REQ_0428`
//! (reply-stream inbound saturation).
//!
//! These tests verify the bridge-level contract — the underlying
//! `OutboundBridge` / `InboundBridge` types from `crate::bridge` —
//! plus the integration with [`ZenohHealthMonitor`]:
//! [`BridgedInboundPublish`] / [`BridgedCorrelatedPublish`] route
//! [`InboundOutcome::Dropped`]'s cumulative count into
//! [`ZenohHealthMonitor::record_inbound_drop`], which emits a single
//! `ConnectorHealth::Degraded { reason: "dropped N inbound frames" }`
//! transition once the configured threshold is crossed.
//!
//! The end-to-end plugin → gateway → session → plugin pipeline can
//! also saturate (under the right configuration), but iceoryx2's
//! internal queue depth makes that path non-deterministic without
//! bespoke pacing. The bridge-level test pins the contract.

use std::sync::Arc;

use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};
use taktora_connector_zenoh::{
    BridgedCorrelatedPublish, BridgedInboundPublish, CorrelatedPublish, InboundBridge,
    InboundOutcome, InboundPublish, OutboundBridge, OutboundError, QueryId, ZenohHealthMonitor,
};

#[test]
fn outbound_bridge_full_returns_backpressure_with_payload() {
    // REQ_0405: when the outbound bridge is full, `try_send` returns
    // `BackPressure(T)` carrying the rejected payload.
    let bridge: OutboundBridge<u32> = OutboundBridge::new(2);
    bridge.try_send(1).expect("first slot");
    bridge.try_send(2).expect("second slot");

    let err = bridge.try_send(99).expect_err("over capacity");
    match err {
        OutboundError::BackPressure(99) => {}
        other => panic!("expected BackPressure(99), got {other:?}"),
    }
}

#[test]
fn outbound_bridge_drains_then_recovers() {
    // REQ_0405 follow-up: after draining, the bridge accepts again.
    let bridge: OutboundBridge<u32> = OutboundBridge::new(1);
    bridge.try_send(10).unwrap();
    assert!(bridge.try_send(20).is_err()); // BackPressure
    bridge.try_recv().expect("drained 10");
    bridge.try_send(20).expect("now accepts");
}

#[test]
fn inbound_bridge_full_records_drop_count() {
    // REQ_0406: when the inbound bridge is full, `try_send` returns
    // `Dropped { count }` reflecting the running drop count.
    let bridge: InboundBridge<u32> = InboundBridge::new(1);
    assert!(matches!(bridge.try_send(1), InboundOutcome::Sent));
    assert!(matches!(
        bridge.try_send(2),
        InboundOutcome::Dropped { count: 1 }
    ));
    assert!(matches!(
        bridge.try_send(3),
        InboundOutcome::Dropped { count: 2 }
    ));
    assert!(matches!(
        bridge.try_send(4),
        InboundOutcome::Dropped { count: 3 }
    ));
    assert_eq!(bridge.dropped_count(), 3);
}

#[test]
fn inbound_bridge_drop_count_persists_after_drain() {
    // REQ_0406 detail: drop count is cumulative across drains.
    let bridge: InboundBridge<u32> = InboundBridge::new(1);
    assert!(matches!(bridge.try_send(1), InboundOutcome::Sent));
    assert!(matches!(
        bridge.try_send(2),
        InboundOutcome::Dropped { count: 1 }
    ));
    bridge.try_recv(); // drain
    assert!(matches!(bridge.try_send(3), InboundOutcome::Sent));
    assert!(matches!(
        bridge.try_send(4),
        InboundOutcome::Dropped { count: 2 }
    ));
    assert_eq!(bridge.dropped_count(), 2);
}

/// `REQ_0406`: subscriber-side inbound saturation. Driving the
/// per-channel bridge into overflow accounts the drops via
/// `ZenohHealthMonitor::record_inbound_drop`, which emits a single
/// `Up → Degraded` transition with a reason mentioning "dropped".
#[test]
fn subscriber_overflow_emits_degraded_transition() {
    let health = Arc::new(ZenohHealthMonitor::new());
    // Force the monitor to `Up` so the drops-driven transition has a
    // legal source state. Bridge `Connecting → Up` via the same
    // `transition_to` path the real session watcher uses.
    health.transition_to(ConnectorHealth::Up).unwrap();
    let sub = health.subscribe();
    while sub.try_recv().is_ok() {}

    // Capacity 1, threshold 2 — drive 3+ inbound frames into the
    // bridge to trigger overflow.
    let publish: BridgedInboundPublish<8> =
        BridgedInboundPublish::without_transport(1, Arc::clone(&health), 2);

    // Manually fill + overflow the bridge so the wrapper's next
    // publish observes `Dropped`.
    let bridge = publish.bridge();
    let _ = bridge.try_send(()); // fills capacity
    let _ = bridge.try_send(()); // drop #1
    let _ = bridge.try_send(()); // drop #2
    // Now the wrapper's try_send returns Dropped { count: 3 }, which
    // crosses the threshold of 2 and emits Degraded.
    publish
        .publish_bytes(b"sample")
        .expect("publish returns Ok");

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

/// `REQ_0428`: reply-stream inbound saturation. The same drop +
/// Degraded contract holds for the gateway → plugin reply path of a
/// querier channel ([`BridgedCorrelatedPublish`]).
#[test]
fn reply_stream_overflow_emits_degraded_transition() {
    let health = Arc::new(ZenohHealthMonitor::new());
    health.transition_to(ConnectorHealth::Up).unwrap();
    let sub = health.subscribe();
    while sub.try_recv().is_ok() {}

    let publish: BridgedCorrelatedPublish<8> =
        BridgedCorrelatedPublish::without_transport(1, Arc::clone(&health), 2);

    // Force overflow on the underlying bridge.
    let bridge = publish.bridge();
    let _ = bridge.try_send(());
    let _ = bridge.try_send(());
    let _ = bridge.try_send(());

    let id = QueryId([0u8; 32]);
    publish
        .publish_with_correlation(id, b"reply")
        .expect("publish returns Ok");

    assert!(health.degraded_due_to_drops());
    assert_eq!(health.current().kind(), ConnectorHealthKind::Degraded);
    let evt = sub.try_recv().expect("Degraded broadcast");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
}

/// Recovery: once the session watcher transitions the monitor back to
/// `Up` (via `transition_to(Up)` from `Degraded`), the drops-latch
/// re-arms.
#[test]
fn recovery_to_up_rearms_drops_latch() {
    let health = Arc::new(ZenohHealthMonitor::new());
    health.transition_to(ConnectorHealth::Up).unwrap();

    let evt = health
        .record_inbound_drop(5, 1)
        .expect("crossing threshold emits Degraded");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
    assert!(health.degraded_due_to_drops());

    health
        .transition_to(ConnectorHealth::Up)
        .expect("Degraded → Up is legal");
    assert!(!health.degraded_due_to_drops());

    let evt2 = health
        .record_inbound_drop(7, 1)
        .expect("second crossing re-emits Degraded");
    assert_eq!(evt2.to.kind(), ConnectorHealthKind::Degraded);
}
