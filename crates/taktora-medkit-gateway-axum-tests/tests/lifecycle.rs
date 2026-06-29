//! Integration tests for the lifecycle-status write family (`REQ_0975`).
//!
//! Drives a live axum server whose write seam is an in-memory
//! [`SimActionSink`] (no real effect) and exercises the SOVD lifecycle surface
//! over real TCP: read the default status (`GET …/status` → `200` `running`),
//! request a `shutdown` transition (`PUT` → `202` `stopped`) and read it back,
//! then `start` again (`PUT` → `202` `running`).
//!
//! - `TEST_0950` — GET `…/status` on a fresh entity → `200` `running`;
//!   PUT `…/status/shutdown` → `202` `stopped` then GET reads `stopped`;
//!   PUT `…/status/start` → `202` `running`.

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

/// Spawn a live server backed by a default (empty) write seam; lifecycle status
/// defaults to `running` and transitions are tracked at runtime, so no
/// pre-configuration is needed.
async fn spawn() -> SocketAddr {
    let view = Arc::new(Gateway::new(demo::provider()).view());
    let sink: Arc<dyn taktora_medkit_provider::ActionSink> = Arc::new(SimActionSink::new());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener_with_actions(listener, view, &GatewayConfig::default(), sink).await;
    });
    addr
}

/// `TEST_0950` — the lifecycle-status surface over the wire.
#[tokio::test]
async fn lifecycle_status_over_http() {
    let addr = spawn().await;
    let base = "/api/v1/apps/gw/status";

    // A fresh entity reads as `running`.
    let fresh = get(addr, base).await;
    assert_eq!(fresh.status, 200, "get: {}", fresh.body);
    assert_eq!(json(&fresh.body)["status"], "running");

    // PUT shutdown -> 202 stopped; GET reads it back.
    let shutdown = request(addr, "PUT", &format!("{base}/shutdown"), None).await;
    assert_eq!(shutdown.status, 202, "shutdown: {}", shutdown.body);
    assert_eq!(json(&shutdown.body)["status"], "stopped");
    assert_eq!(json(&get(addr, base).await.body)["status"], "stopped");

    // PUT start -> 202 running.
    let start = request(addr, "PUT", &format!("{base}/start"), None).await;
    assert_eq!(start.status, 202, "start: {}", start.body);
    assert_eq!(json(&start.body)["status"], "running");
}
