//! `TEST_0917` — drive a **live** axum gateway (the skeleton from #81) over the
//! connector binding and assert, over real TCP, that the Component and its DTCs
//! surface through the running gateway and that each confirmed DTC carries a
//! last-sample freeze-frame under the contract's `snapshots` /
//! `extended_data_records` shape.
//!
//! The server binds `127.0.0.1:0` (ephemeral) and the test reads back the real
//! port, so the suite is parallel-safe under CI's serial run.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use taktora_connector_core::health::ConnectorHealth;
use taktora_medkit_binding_connector::{DTC_DEGRADED, DTC_NOT_OPERATIONAL, MedkitProvider};
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, ephemeral_listener, serve_listener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const COMPONENT: &str = "component:ethercat0";

/// Build a binding driven to `Up → Degraded → Down` (leaving an active Critical
/// DTC, a healed Degraded DTC, and a captured freeze-frame), then spawn a live
/// server over its current snapshot and return the bound address.
async fn spawn() -> SocketAddr {
    let binding = MedkitProvider::new(COMPONENT, "EtherCAT bus 0");
    binding
        .observe_sample(serde_json::json!({ "wkc": 2, "expected_wkc": 3, "al_state": "SAFEOP" }));
    binding.apply(&ConnectorHealth::Up, 1.0);
    binding.apply(
        &ConnectorHealth::Degraded {
            reason: "wkc low".to_owned(),
        },
        2.0,
    );
    binding.apply(
        &ConnectorHealth::Down {
            reason: "link lost".to_owned(),
            since: Instant::now(),
        },
        3.0,
    );

    let gateway = Gateway::new(binding);
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind ephemeral port");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// Minimal HTTP/1.1 GET (one request per connection, `Connection: close`), so
/// the body is everything up to EOF. Avoids a heavyweight HTTP-client dep.
async fn get_json(addr: SocketAddr, path: &str) -> Value {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (_, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    serde_json::from_str(body).unwrap_or_else(|e| panic!("parse body of {path}: {e}\n{body}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn component_and_dtcs_surface_through_running_gateway() {
    let addr = spawn().await;

    // The Component surfaces in the top-level list.
    let components = get_json(addr, "/api/v1/components").await;
    let ids: Vec<&str> = components["items"]
        .as_array()
        .expect("items array")
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(ids.contains(&COMPONENT), "component missing: {ids:?}");

    // The entity-scoped fault list carries both DTCs.
    let faults = get_json(addr, &format!("/api/v1/components/{COMPONENT}/faults")).await;
    let codes: Vec<&str> = faults["items"]
        .as_array()
        .expect("fault items")
        .iter()
        .filter_map(|f| f["fault_code"].as_str())
        .collect();
    assert!(
        codes.contains(&DTC_NOT_OPERATIONAL),
        "missing critical DTC: {codes:?}"
    );
    assert!(
        codes.contains(&DTC_DEGRADED),
        "missing degraded DTC: {codes:?}"
    );

    // The single-fault detail exposes the contract's camelCase status sub-object.
    let detail = get_json(
        addr,
        &format!("/api/v1/components/{COMPONENT}/faults/{DTC_NOT_OPERATIONAL}"),
    )
    .await;
    assert_eq!(detail["item"]["code"], Value::from(DTC_NOT_OPERATIONAL));
    assert_eq!(detail["item"]["severity"], Value::from(3));
    assert_eq!(detail["item"]["status"]["confirmedDTC"], Value::from("1"));
    assert_eq!(detail["item"]["status"]["testFailed"], Value::from("1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmed_dtc_freeze_frame_reachable_under_fault_detail() {
    // `TEST_0918` — the confirmed DTC's freeze-frame is reachable through the
    // proper SOVD fault-detail endpoint (`…/faults/{code}`), carried under the
    // contract's `snapshots` / `extended_data_records` (`REQ_0929`), not only the
    // `…/data` workaround.
    let addr = spawn().await;

    let detail = get_json(
        addr,
        &format!("/api/v1/components/{COMPONENT}/faults/{DTC_NOT_OPERATIONAL}"),
    )
    .await;

    let env = &detail["environment_data"];
    let records = &env["extended_data_records"];
    assert!(records["first_occurrence"].is_string());
    assert!(records["last_occurrence"].is_string());

    let frame = &env["snapshots"][0];
    assert_eq!(frame["type"], Value::from("freeze_frame"));
    // The captured last-sample is the connector hook sample observed before
    // confirmation.
    assert_eq!(frame["x-medkit"]["full_data"]["wkc"], Value::from(2));
    assert_eq!(
        frame["x-medkit"]["full_data"]["expected_wkc"],
        Value::from(3)
    );
    assert!(frame["x-medkit"]["captured_at"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmed_dtc_freeze_frame_reachable_under_data() {
    let addr = spawn().await;

    // The binding surfaces each DTC's freeze-frame environment data under the
    // Component's `…/data`, reachable through the running gateway.
    let env = get_json(
        addr,
        &format!("/api/v1/components/{COMPONENT}/data/dtcs/{DTC_NOT_OPERATIONAL}"),
    )
    .await;

    // Contract freeze-frame shape: snapshots[] + extended_data_records.
    let records = &env["extended_data_records"];
    assert!(records["first_occurrence"].is_string());
    assert!(records["last_occurrence"].is_string());

    let frame = &env["snapshots"][0];
    assert_eq!(frame["type"], Value::from("freeze_frame"));
    // The captured last-sample is the connector hook sample observed before
    // confirmation.
    assert_eq!(frame["x-medkit"]["full_data"]["wkc"], Value::from(2));
    assert_eq!(
        frame["x-medkit"]["full_data"]["expected_wkc"],
        Value::from(3)
    );
    assert!(frame["x-medkit"]["captured_at"].is_string());
}
