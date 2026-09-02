//! Integration tests for the operations write family (`REQ_0969`, `REQ_0970`).
//!
//! Drives a live axum server whose write seam is a configured in-memory
//! [`SimActionSink`] (no real effect) and exercises the full SOVD async
//! execution lifecycle over real TCP: list operations, start an execution
//! (`202`), poll it, list executions, cancel (`204`), and the `404` paths.
//!
//! - `TEST_0943` — the operations catalogue lists and details configured ops.
//! - `TEST_0944` — start → `202` completed execution; poll and list find it;
//!   cancel removes it; a subsequent poll is `404`.
//! - `TEST_0945` — an unknown operation is `404` on both start and detail.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{
    GatewayConfig, demo, ephemeral_listener, serve_listener_with_actions,
};
use taktora_medkit_model::EntityKind;
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

/// Spawn a live server whose write seam has one operation `reset` on app `gw`.
async fn spawn() -> SocketAddr {
    let view = Arc::new(Gateway::new(demo::provider()).view());
    let sink: Arc<dyn taktora_medkit_provider::ActionSink> =
        Arc::new(SimActionSink::new().with_operation(EntityKind::App, "gw", "reset"));
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener_with_actions(listener, view, &GatewayConfig::default(), sink).await;
    });
    addr
}

/// `TEST_0943` — the operations catalogue lists and details configured ops.
// @need-ids: TEST_0943
#[tokio::test]
async fn operations_catalogue_lists_and_details() {
    let addr = spawn().await;

    let listed = json(&get(addr, "/api/v1/apps/gw/operations").await.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], "reset");

    let detail = get(addr, "/api/v1/apps/gw/operations/reset").await;
    assert_eq!(detail.status, 200);
    assert_eq!(json(&detail.body)["name"], "reset");

    // A resource with no configured operations has an empty catalogue.
    let empty = json(&get(addr, "/api/v1/apps/other/operations").await.body);
    assert_eq!(empty["x-medkit"]["total_count"], 0);
}

/// `TEST_0944` — the full async execution lifecycle over the wire.
// @need-ids: TEST_0944
#[tokio::test]
async fn execution_lifecycle_over_http() {
    let addr = spawn().await;
    let execs = "/api/v1/apps/gw/operations/reset/executions";

    // POST starts an execution -> 202 with the (sim-completed) execution.
    let started = request(addr, "POST", execs, Some(r#"{"force":true}"#)).await;
    assert_eq!(started.status, 202, "start: {}", started.body);
    let started = json(&started.body);
    let id = started["id"].as_str().expect("execution id").to_owned();
    assert_eq!(started["status"], "completed");
    assert_eq!(started["result"]["echo"]["force"], true);

    // GET the execution -> completed.
    let polled = get(addr, &format!("{execs}/{id}")).await;
    assert_eq!(polled.status, 200);
    assert_eq!(json(&polled.body)["status"], "completed");

    // The executions list contains it.
    let listed = json(&get(addr, execs).await.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], id.as_str());

    // DELETE cancels/removes it -> 204; a subsequent GET is 404.
    let cancelled = request(addr, "DELETE", &format!("{execs}/{id}"), None).await;
    assert_eq!(cancelled.status, 204);
    let gone = get(addr, &format!("{execs}/{id}")).await;
    assert_eq!(gone.status, 404);
}

/// `TEST_0945` — an unknown operation is `404` on both detail and start.
// @need-ids: TEST_0945
#[tokio::test]
async fn unknown_operation_is_404() {
    let addr = spawn().await;

    let detail = get(addr, "/api/v1/apps/gw/operations/nope").await;
    assert_eq!(detail.status, 404);

    let started = request(
        addr,
        "POST",
        "/api/v1/apps/gw/operations/nope/executions",
        Some("{}"),
    )
    .await;
    assert_eq!(started.status, 404, "start of unknown op: {}", started.body);
}
