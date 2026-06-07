//! TEST_0864 (ethercat half) — health subscriptions are independent
//! broadcast streams (`REQ_0847`).
//!
//! Found live on the WAGO bench (#60): `subscribe()` used to hand out
//! clones of ONE crossbeam receiver — competing consumers, so a
//! fast-polling second subscriber silently stole every event from the
//! health pump. Every subscription must observe every transition.

#![allow(clippy::doc_markdown)]

use std::time::Instant;

use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};
use taktora_connector_ethercat::EthercatHealthMonitor;

#[test]
fn every_subscriber_observes_every_transition() {
    let monitor = EthercatHealthMonitor::new();
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
    let monitor = EthercatHealthMonitor::new();
    monitor
        .transition_to(ConnectorHealth::Up)
        .expect("legal transition");

    // Subscribed after the fact: no stale history replay.
    let late = monitor.subscribe();
    assert!(late.try_recv().is_err(), "no pre-subscription events");

    monitor
        .transition_to(ConnectorHealth::Down {
            reason: "test".into(),
            since: Instant::now(),
        })
        .expect("legal transition");
    assert_eq!(
        late.try_recv().expect("future event observed").to.kind(),
        ConnectorHealthKind::Down
    );
}

#[test]
fn transitions_succeed_with_zero_subscribers() {
    let monitor = EthercatHealthMonitor::new();
    // No subscriber exists; the transition must still succeed and be
    // reflected in current().
    monitor
        .transition_to(ConnectorHealth::Up)
        .expect("transition with no subscribers");
    assert_eq!(monitor.current().kind(), ConnectorHealthKind::Up);
}
