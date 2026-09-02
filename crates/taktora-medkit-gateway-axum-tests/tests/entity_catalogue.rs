//! Single-entity capability catalogue tests: drive a **live** axum server over
//! the demo provider and assert the verbose `GET /{collection}/{id}` document
//! carries the SOVD capability catalogue (`REQ_0979`).
//!
//! - `TEST_0954` — an app detail document carries a `capabilities` array (each
//!   entry an entity-scoped `{ "name", "href" }`), the flat top-level
//!   sub-resource href keys, and the baseline `_links.self`/`_links.collection`;
//!   a function detail document carries the narrower per-kind set.

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, demo, ephemeral_listener, serve_listener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Spawn a live server over the demo provider.
async fn spawn() -> SocketAddr {
    let gateway = Gateway::new(demo::provider());
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind ephemeral port");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// Minimal HTTP/1.1 GET returning `(status, parsed JSON)`; one request per
/// connection (`Connection: close`).
async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("status code");
    let value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("{path}: body is not JSON ({e}): {body}"));
    (status, value)
}

/// Collect the `name`s of a detail document's `capabilities` array.
fn capability_names(doc: &Value) -> Vec<&str> {
    doc["capabilities"]
        .as_array()
        .expect("capabilities array")
        .iter()
        .map(|e| e["name"].as_str().expect("capability name"))
        .collect()
}

/// `TEST_0954` — an app detail document advertises the full SOVD catalogue.
// @need-ids: TEST_0954
#[tokio::test]
async fn app_detail_carries_capability_catalogue() {
    let addr = spawn().await;
    let (status, doc) = get_json(addr, "/api/v1/apps/ros2_medkit_gateway").await;
    assert_eq!(status, 200, "app detail must be 200: {doc}");

    // The `_links` baseline survives the enrichment.
    assert_eq!(
        doc["_links"]["self"], "/api/v1/apps/ros2_medkit_gateway",
        "links.self"
    );
    assert_eq!(
        doc["_links"]["collection"], "/api/v1/apps",
        "links.collection"
    );

    // The catalogue array lists every sub-resource the kind exposes, each with
    // an entity-scoped href.
    let names = capability_names(&doc);
    for expected in [
        "data",
        "operations",
        "configurations",
        "faults",
        "logs",
        "bulk-data",
        "cyclic-subscriptions",
        "triggers",
    ] {
        assert!(
            names.contains(&expected),
            "app capabilities must include `{expected}`: {names:?}"
        );
    }
    let entries = doc["capabilities"].as_array().expect("capabilities array");
    for entry in entries {
        let href = entry["href"].as_str().expect("capability href");
        assert!(
            href.starts_with("/api/v1/apps/ros2_medkit_gateway/"),
            "capability href must be entity-scoped: {href}"
        );
    }

    // The flat top-level sub-resource href keys are present as string URLs.
    assert_eq!(
        doc["bulk-data"], "/api/v1/apps/ros2_medkit_gateway/bulk-data",
        "flat bulk-data href"
    );
    assert!(
        doc["data"].as_str().is_some() && doc["faults"].as_str().is_some(),
        "flat data/faults hrefs must be string URLs"
    );
}

/// `TEST_0954` — a function detail document carries the narrower per-kind set:
/// its catalogue omits the families functions do not expose (`locks`, `status`).
// @need-ids: TEST_0954
#[tokio::test]
async fn function_detail_carries_narrower_catalogue() {
    let addr = spawn().await;
    let (status, doc) = get_json(addr, "/api/v1/functions/root").await;
    assert_eq!(status, 200, "function detail must be 200: {doc}");

    assert_eq!(
        doc["_links"]["self"], "/api/v1/functions/root",
        "links.self"
    );
    assert_eq!(
        doc["_links"]["collection"], "/api/v1/functions",
        "links.collection"
    );

    let names = capability_names(&doc);
    // The narrower set: a function still exposes its data-family sub-resources
    // and its `hosts` relation…
    for expected in ["hosts", "data", "operations", "faults", "logs"] {
        assert!(
            names.contains(&expected),
            "function capabilities must include `{expected}`: {names:?}"
        );
    }
    // …but not the lifecycle/lock families that only apps and components carry.
    for absent in ["locks", "status"] {
        assert!(
            !names.contains(&absent),
            "function capabilities must NOT include `{absent}`: {names:?}"
        );
    }
}
