//! Outbound-bridge saturation contract (`REQ_0260`).
//!
//! When the outbound bridge is full, [`BridgedOutbound::try_send`] returns
//! [`ConnectorError::BackPressure`] and folds a single
//! `ConnectorHealth::Degraded` transition into the shared
//! [`MqttHealthMonitor`]. Mirrors `taktora-connector-zenoh`'s bridge-level
//! saturation coverage: the end-to-end plugin → gateway → session pipeline
//! can also saturate, but iceoryx2's internal queue depth makes that path
//! non-deterministic, so the bridge-level test pins the contract.

use std::sync::Arc;

use taktora_connector_core::{ConnectorError, ConnectorHealth, ConnectorHealthKind};
use taktora_connector_mqtt::{BridgedOutbound, MqttHealthMonitor};

#[test]
fn outbound_saturation_returns_backpressure_and_degrades() {
    // REQ_0260: a full outbound bridge → BackPressure + Degraded.
    let health = Arc::new(MqttHealthMonitor::new());
    let sub = health.subscribe();
    let gate: BridgedOutbound<u32> = BridgedOutbound::new(1, Arc::clone(&health));

    gate.try_send(1).expect("first slot accepts");
    let err = gate.try_send(2).expect_err("over capacity rejects");
    assert!(
        matches!(err, ConnectorError::BackPressure),
        "expected BackPressure, got {err:?}"
    );

    assert!(health.degraded_due_to_backpressure());
    assert_eq!(health.current().kind(), ConnectorHealthKind::Degraded);

    let evt = sub.try_recv().expect("Degraded transition broadcast");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
    match &evt.to {
        ConnectorHealth::Degraded { reason } => {
            assert!(
                reason.contains("backpressure"),
                "reason {reason:?} must mention backpressure"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

#[test]
fn outbound_bridge_drains_then_recovers() {
    // After draining a slot the gate accepts again; health stays Degraded
    // until an explicit recovery transition.
    let health = Arc::new(MqttHealthMonitor::new());
    let gate: BridgedOutbound<u32> = BridgedOutbound::new(1, Arc::clone(&health));

    gate.try_send(10).expect("first slot");
    assert!(gate.try_send(20).is_err(), "full → BackPressure");
    gate.bridge().try_recv().expect("drain frees a slot");
    gate.try_send(20).expect("now accepts after drain");
}

#[test]
fn backpressure_degraded_latches_and_rearms_on_recovery() {
    // REQ_0260 latch: repeated saturation emits Degraded only once; a
    // recovery to Up re-arms the latch.
    let health = MqttHealthMonitor::new();

    let first = health
        .record_outbound_backpressure()
        .expect("first crossing emits Degraded");
    assert_eq!(first.to.kind(), ConnectorHealthKind::Degraded);

    // Latched: further crossings emit nothing.
    assert!(health.record_outbound_backpressure().is_none());
    assert!(health.record_outbound_backpressure().is_none());

    // Recovery to Up re-arms the latch.
    health
        .transition_to(ConnectorHealth::Up)
        .expect("Degraded → Up is legal");
    assert!(!health.degraded_due_to_backpressure());

    let second = health
        .record_outbound_backpressure()
        .expect("second crossing after recovery re-emits Degraded");
    assert_eq!(second.to.kind(), ConnectorHealthKind::Degraded);
}
