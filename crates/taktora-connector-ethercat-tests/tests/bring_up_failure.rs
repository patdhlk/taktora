//! TEST_0858 — bring-up failure is observable via the health
//! subscription (`REQ_0842`).
//!
//! `EthercatConnector::register_with` runs bring-up inside a spawned
//! gateway task. A failure there used to be dropped on the floor
//! (`let _ = e;`), leaving the connector in `Connecting` forever with
//! no observable signal — every diagnosis started from a silent hang.
//! The connector must instead drive the health monitor to a terminal
//! `Down` carrying the bring-up error, so the existing health
//! subscription is the single place observers learn about it.

#![allow(clippy::doc_markdown)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ConnectorHealth, ConnectorHealthKind};
use taktora_connector_ethercat::connector::EthercatState;
use taktora_connector_ethercat::{EthercatConnector, EthercatConnectorOptions, MockBusDriver};
use taktora_connector_host::Connector;
use taktora_executor::Executor;

#[test]
fn failed_bring_up_transitions_health_to_down() {
    let opts = EthercatConnectorOptions::builder().build();
    let state = Arc::new(EthercatState::new(opts));
    let mock = MockBusDriver::new().failing_bring_up("simulated bring-up fault");
    let mut connector =
        EthercatConnector::new(state, mock, JsonCodec::new()).expect("construct connector");

    let sub = connector.subscribe_health();
    let mut exec = Executor::builder()
        .worker_threads(1)
        .build()
        .expect("build executor");
    connector
        .register_with(&mut exec)
        .expect("register_with succeeds — the failure happens async");

    // Bring-up runs on the gateway's tokio runtime; poll the
    // subscription until the terminal transition lands.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = sub.try_next().expect("health channel alive") {
            assert_eq!(event.from.kind(), ConnectorHealthKind::Connecting);
            match &event.to {
                ConnectorHealth::Down { reason, .. } => {
                    assert!(
                        reason.contains("bring-up failed"),
                        "reason must name the failing phase: {reason}"
                    );
                    assert!(
                        reason.contains("simulated bring-up fault"),
                        "reason must carry the driver error: {reason}"
                    );
                }
                other => panic!("expected terminal Down, got {other:?}"),
            }
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no health transition observed within 5s — bring-up failure was swallowed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
