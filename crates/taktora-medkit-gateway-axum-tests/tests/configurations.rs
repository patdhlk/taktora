//! Integration tests for the configurations write family (`REQ_0971`).
//!
//! Drives a live axum server whose write seam is an in-memory
//! [`SimActionSink`] (no real effect) and exercises the full SOVD configuration
//! lifecycle over real TCP: upsert (`PUT` → `200`), read it back, list it,
//! overwrite, delete one (`204`) and the `404` path, and delete-all (`204`).
//!
//! - `TEST_0946` — PUT a config → `200`; GET it → `200` with the value; the list
//!   shows `total_count` 1; PUT again overwrites; DELETE one → `204` then GET is
//!   `404`; DELETE all → `204`.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{
    GatewayConfig, demo, ephemeral_listener, serve_listener_with_actions,
};
use taktora_medkit_provider::SimActionSink;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Response {
    status: u16,
    body: String,
}

async fn request(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> Response {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
    if let Some(b) = body {
        let _ = write!(
            head,
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        );
    }
    head.push_str("Connection: close\r\n\r\n");
    let req = format!("{head}{}", body.unwrap_or(""));
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
    Response {
        status,
        body: body.to_owned(),
    }
}

async fn get(addr: SocketAddr, path: &str) -> Response {
    request(addr, "GET", path, None).await
}

fn json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not JSON ({e}): {body}"))
}

/// Spawn a live server backed by a default (empty) write seam; configs are
/// upserted at runtime, so no pre-configuration is needed.
async fn spawn() -> SocketAddr {
    let view = Arc::new(Gateway::new(demo::provider()).view());
    let sink: Arc<dyn taktora_medkit_provider::ActionSink> = Arc::new(SimActionSink::new());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener_with_actions(listener, view, &GatewayConfig::default(), sink).await;
    });
    addr
}

/// `TEST_0946` — the full configuration lifecycle over the wire.
#[tokio::test]
async fn configuration_lifecycle_over_http() {
    let addr = spawn().await;
    let base = "/api/v1/apps/gw/configurations";
    let one = format!("{base}/rate");

    // Unset config is 404; the list is empty.
    assert_eq!(get(addr, &one).await.status, 404);
    let empty = json(&get(addr, base).await.body);
    assert_eq!(empty["x-medkit"]["total_count"], 0);

    // PUT upserts -> 200 with the stored entry.
    let put = request(addr, "PUT", &one, Some(r#"{"hz":50}"#)).await;
    assert_eq!(put.status, 200, "put: {}", put.body);
    let stored = json(&put.body);
    assert_eq!(stored["id"], "rate");
    assert_eq!(stored["value"]["hz"], 50);

    // GET reads it back.
    let got = get(addr, &one).await;
    assert_eq!(got.status, 200);
    assert_eq!(json(&got.body)["value"]["hz"], 50);

    // The list shows it.
    let listed = json(&get(addr, base).await.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], "rate");

    // PUT again overwrites the value.
    let updated = request(addr, "PUT", &one, Some(r#"{"hz":100}"#)).await;
    assert_eq!(updated.status, 200);
    assert_eq!(json(&updated.body)["value"]["hz"], 100);
    assert_eq!(json(&get(addr, &one).await.body)["value"]["hz"], 100);

    // DELETE one -> 204; a subsequent GET is 404.
    let deleted = request(addr, "DELETE", &one, None).await;
    assert_eq!(deleted.status, 204);
    assert_eq!(get(addr, &one).await.status, 404);

    // DELETE all -> 204 (always), and the list is empty.
    request(addr, "PUT", &format!("{base}/a"), Some("1")).await;
    request(addr, "PUT", &format!("{base}/b"), Some("2")).await;
    let cleared = request(addr, "DELETE", base, None).await;
    assert_eq!(cleared.status, 204);
    assert_eq!(
        json(&get(addr, base).await.body)["x-medkit"]["total_count"],
        0
    );
}
