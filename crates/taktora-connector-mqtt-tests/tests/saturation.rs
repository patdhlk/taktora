//! Inbound-bridge saturation (`REQ_0261`).
//!
//! When the per-channel inbound bridge is full the gateway drops the
//! offending frame, increments the cumulative drop counter exposed via
//! `InboundOutcome::Dropped { count }`, and emits a single
//! `ConnectorHealth::Degraded { reason: "dropped N inbound frames" }`
//! transition once the count crosses the configured
//! `inbound_drop_threshold`. Mirrors the Zenoh connector's bridge-level
//! saturation coverage — the iceoryx2 end-to-end queue depth makes the
//! full pipeline non-deterministic, so the bridge contract is pinned here.

use std::sync::Arc;

use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};
use taktora_connector_mqtt::{BridgedInboundPublish, InboundPublish, MqttHealthMonitor};

/// `REQ_0261`: driving the per-channel inbound bridge into overflow
/// accounts the drops and emits exactly one `Up → Degraded` transition
/// whose reason mentions the dropped frames, once the threshold is
/// crossed.
#[test]
fn inbound_overflow_emits_single_degraded_transition() {
    let health = Arc::new(MqttHealthMonitor::new());
    // Give the drop-driven transition a legal source state.
    health.transition_to(ConnectorHealth::Up).unwrap();
    let sub = health.subscribe();
    while sub.try_recv().is_ok() {}

    // Capacity 1, threshold 2. Fill + overflow the bridge directly so the
    // wrapper's next publish observes `Dropped`.
    let publish: BridgedInboundPublish<8> =
        BridgedInboundPublish::without_transport(1, Arc::clone(&health), 2);
    let bridge = publish.bridge();
    let _ = bridge.try_send(()); // fills capacity 1
    let _ = bridge.try_send(()); // drop #1
    let _ = bridge.try_send(()); // drop #2

    // The wrapper's own send returns `Dropped { count: 3 }`, crossing the
    // threshold of 2 → one Degraded transition.
    publish.publish_bytes(b"sample").expect("publish returns Ok");

    match health.current() {
        ConnectorHealth::Degraded { reason } => {
            assert!(
                reason.contains("dropped"),
                "reason should mention dropped frames, got {reason:?}"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }

    let event = sub.try_recv().expect("Degraded broadcast to subscriber");
    assert_eq!(event.to.kind(), ConnectorHealthKind::Degraded);
    // Latched: no further Degraded events until recovery.
    assert!(sub.try_recv().is_err(), "at most one Degraded transition");
}
