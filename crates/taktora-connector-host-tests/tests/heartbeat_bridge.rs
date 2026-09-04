//! Integration test for `HeartbeatHealthBridge`: verifies that the bridge
//! forwards executor heartbeat ticks onto a `HealthEvent` channel.
//! Supports `TSR_0010` / `AOU_0003`.

#![allow(clippy::doc_markdown)]

use std::sync::Arc;
use std::time::{Duration, Instant};

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
    // Drive to a fixed number of forwarded ticks rather than asserting a rate
    // over a fixed wall-clock window. A transient CI scheduler stall can
    // swallow most of a short window (observed: 1 tick in 150 ms on a loaded
    // macOS runner), so a rate assertion is inherently flaky. The dispatch loop
    // keeps iterating across a stall — the stall only lengthens one wait — so a
    // count-driven run is deterministic. The wall-clock safety cap turns a
    // genuinely dead heartbeat into a clean assertion failure, not a hang.
    let target_events: usize = 5;
    let safety_cap = Duration::from_secs(10);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(bridge as Arc<dyn Observer>)
        .build()
        .unwrap();

    let start = Instant::now();
    exec.run_until(|| health_rx.len() >= target_events || start.elapsed() > safety_cap)
        .unwrap();

    // Collect the health events.
    let events: Vec<HealthEvent> = health_rx.try_iter().collect();

    assert!(
        events.len() >= target_events,
        "expected at least {target_events} health events, got {} (heartbeat not firing?)",
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
    // Count-driven with a safety cap, for the same CI-stall reason documented on
    // bridge_forwards_ticks_to_health_channel. Verifies the WaitSet wait is
    // bounded by the heartbeat deadline even with no tasks: ticks keep arriving.
    let target_events: usize = 3;
    let safety_cap = Duration::from_secs(10);

    // No tasks registered.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .heartbeat(period)
        .observer(bridge as Arc<dyn Observer>)
        .build()
        .unwrap();

    let start = Instant::now();
    exec.run_until(|| health_rx.len() >= target_events || start.elapsed() > safety_cap)
        .unwrap();

    let events: Vec<HealthEvent> = health_rx.try_iter().collect();

    assert!(
        events.len() >= target_events,
        "expected at least {target_events} events with no tasks, got {} (heartbeat not firing?)",
        events.len()
    );
}
