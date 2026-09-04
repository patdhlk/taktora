//! Integration test for `HeartbeatHealthBridge`: verifies that the bridge
//! forwards executor heartbeat ticks onto a `HealthEvent` channel.
//! Supports `TSR_0010` / `AOU_0003`.

#![allow(clippy::doc_markdown)]

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::unbounded;
use taktora_connector_core::{ConnectorHealth, HealthEvent};
use taktora_connector_host::HeartbeatHealthBridge;
use taktora_executor::{Executor, Observer};

/// Verify that the bridge forwards heartbeat ticks as `HealthEvent`s when
/// wired as the executor's observer.
#[test]
fn bridge_forwards_ticks_to_health_channel() {
    let (health_tx, health_rx) = unbounded();
    let bridge = Arc::new(HeartbeatHealthBridge::new(health_tx));

    let period = Duration::from_millis(20);
    let run_duration = Duration::from_millis(150);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(bridge as Arc<dyn Observer>)
        .build()
        .unwrap();

    exec.run_for(run_duration).unwrap();

    // Collect the health events.
    let events: Vec<HealthEvent> = health_rx.try_iter().collect();

    // We should have received at least (run_duration / period) - 1 events.
    let expected_min = (run_duration.as_millis() / period.as_millis()) - 1;
    assert!(
        events.len() as u128 >= expected_min,
        "expected at least {expected_min} health events, got {}",
        events.len()
    );

    // Every event should be from Up to Up (liveness heartbeat).
    for event in &events {
        assert!(matches!(event.from, ConnectorHealth::Up));
        assert!(matches!(event.to, ConnectorHealth::Up));
    }
}

/// Verify that the bridge does not panic or block when the health channel
/// receiver is dropped mid-run (graceful degradation).
#[test]
fn bridge_graceful_when_receiver_disconnected() {
    let (health_tx, health_rx) = unbounded();
    let bridge = Arc::new(HeartbeatHealthBridge::new(health_tx));

    let period = Duration::from_millis(20);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(bridge as Arc<dyn Observer>)
        .build()
        .unwrap();

    // Drop the receiver before the executor runs.
    drop(health_rx);

    // The executor should run without panicking.
    exec.run_for(Duration::from_millis(100)).unwrap();
}

/// Verify that the bridge emits events even when the executor has no tasks
/// registered (the heartbeat bounds the WaitSet wait).
#[test]
fn bridge_emits_events_with_no_tasks() {
    let (health_tx, health_rx) = unbounded();
    let bridge = Arc::new(HeartbeatHealthBridge::new(health_tx));

    let period = Duration::from_millis(25);
    let run_duration = Duration::from_millis(125);

    // No tasks registered.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(bridge as Arc<dyn Observer>)
        .build()
        .unwrap();

    exec.run_for(run_duration).unwrap();

    let events: Vec<HealthEvent> = health_rx.try_iter().collect();

    let expected_min = (run_duration.as_millis() / period.as_millis()) - 1;
    assert!(
        events.len() as u128 >= expected_min,
        "expected at least {expected_min} events with no tasks, got {}",
        events.len()
    );
}
