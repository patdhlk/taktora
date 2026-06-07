//! TEST_0864 (zenoh half) — health subscriptions are independent
//! broadcast streams (`REQ_0847`). See #60.

#![allow(clippy::doc_markdown)]

use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};
use taktora_connector_zenoh::ZenohHealthMonitor;

#[test]
fn every_subscriber_observes_every_transition() {
    let monitor = ZenohHealthMonitor::new();
    let sub_a = monitor.subscribe();
    let sub_b = monitor.subscribe();

    monitor
        .transition_to(ConnectorHealth::Up)
        .expect("legal transition");

    for (name, sub) in [("a", &sub_a), ("b", &sub_b)] {
        let event = sub
            .try_recv()
            .unwrap_or_else(|_| panic!("subscriber {name} must observe the transition"));
        assert_eq!(event.to.kind(), ConnectorHealthKind::Up);
    }
}

#[test]
fn subscribers_only_observe_transitions_after_subscribing() {
    let monitor = ZenohHealthMonitor::new();
    monitor
        .transition_to(ConnectorHealth::Up)
        .expect("legal transition");

    let late = monitor.subscribe();
    assert!(late.try_recv().is_err(), "no pre-subscription events");
}
