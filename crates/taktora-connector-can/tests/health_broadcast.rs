//! TEST_0864 (CAN half) — health subscriptions are independent
//! broadcast streams (`REQ_0847`). See #60.

#![allow(clippy::doc_markdown)]

use taktora_connector_can::{CanHealthMonitor, CanIface, IfaceHealthKind};
use taktora_connector_core::ConnectorHealthKind;

#[test]
fn every_subscriber_observes_every_transition() {
    let iface = CanIface::new("can0").expect("iface");
    let monitor = CanHealthMonitor::new(&[iface]);
    let sub_a = monitor.subscribe();
    let sub_b = monitor.subscribe();

    monitor
        .set_iface(iface, IfaceHealthKind::Up)
        .expect("legal transition")
        .expect("aggregate transitions");

    for (name, sub) in [("a", &sub_a), ("b", &sub_b)] {
        let event = sub
            .try_recv()
            .unwrap_or_else(|_| panic!("subscriber {name} must observe the transition"));
        assert_eq!(event.to.kind(), ConnectorHealthKind::Up);
    }
}

#[test]
fn subscribers_only_observe_transitions_after_subscribing() {
    let iface = CanIface::new("can0").expect("iface");
    let monitor = CanHealthMonitor::new(&[iface]);
    monitor
        .set_iface(iface, IfaceHealthKind::Up)
        .expect("legal transition");

    let late = monitor.subscribe();
    assert!(late.try_recv().is_err(), "no pre-subscription events");
}
