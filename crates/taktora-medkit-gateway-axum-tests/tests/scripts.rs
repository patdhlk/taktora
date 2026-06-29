//! Integration tests for the scripts write family (`REQ_0973`).
//!
//! Drives a live axum server whose write seam is an in-memory
//! [`SimActionSink`] (no real effect) and exercises the full SOVD scripts
//! lifecycle over real TCP: upload raw script bytes (`POST` → `201`), read the
//! metadata and the listing, start an execution (`202`), poll it, cancel it
//! (`204`), delete the script (`204`), and the `404` paths.
//!
//! - `TEST_0948` — POST raw bytes → `201` script with an id; GET it → `200`; the
//!   listing shows it; POST an execution → `202` completed; GET it → `200`;
//!   DELETE the execution → `204` then GET → `404`; DELETE the script → `204`
//!   then GET → `404`; starting an execution under an unknown script → `404`.

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
            "Content-Type: application/octet-stream\r\nContent-Length: {}\r\n",
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

/// Spawn a live server backed by a default (empty) write seam; scripts are
/// uploaded at runtime, so no pre-configuration is needed.
async fn spawn() -> SocketAddr {
    let view = Arc::new(Gateway::new(demo::provider()).view());
    let sink: Arc<dyn taktora_medkit_provider::ActionSink> = Arc::new(SimActionSink::new());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener_with_actions(listener, view, &GatewayConfig::default(), sink).await;
    });
    addr
}

/// `TEST_0948` — the full scripts lifecycle (upload + executions) over the wire.
#[tokio::test]
async fn scripts_lifecycle_over_http() {
    let addr = spawn().await;
    let base = "/api/v1/apps/gw/scripts";

    // No scripts on a fresh resource.
    assert_eq!(
        json(&get(addr, base).await.body)["x-medkit"]["total_count"],
        0
    );

    // POST raw bytes -> 201 with a script carrying an id + size.
    let payload = "#!/bin/sh\necho hello scripts";
    let uploaded = request(addr, "POST", base, Some(payload)).await;
    assert_eq!(uploaded.status, 201, "upload: {}", uploaded.body);
    let script = json(&uploaded.body);
    let script_id = script["id"].as_str().expect("script id").to_owned();
    assert!(script_id.starts_with("script-"));
    assert_eq!(script["size"], payload.len());

    // GET the script metadata -> 200.
    let detail = format!("{base}/{script_id}");
    let got = get(addr, &detail).await;
    assert_eq!(got.status, 200);
    assert_eq!(json(&got.body)["id"], script_id.as_str());

    // GET the listing shows the uploaded script.
    let listed = json(&get(addr, base).await.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], script_id.as_str());

    // POST an execution under the script -> 202 with the (sim-completed) exec.
    let execs = format!("{detail}/executions");
    let started = request(addr, "POST", &execs, None).await;
    assert_eq!(started.status, 202, "start: {}", started.body);
    let started = json(&started.body);
    let exec_id = started["id"].as_str().expect("execution id").to_owned();
    assert_eq!(started["status"], "completed");
    assert_eq!(started["result"]["script"], script_id.as_str());

    // GET the execution -> 200 completed.
    let exec = format!("{execs}/{exec_id}");
    let polled = get(addr, &exec).await;
    assert_eq!(polled.status, 200);
    assert_eq!(json(&polled.body)["status"], "completed");

    // DELETE the execution -> 204; a subsequent GET is 404.
    let cancelled = request(addr, "DELETE", &exec, None).await;
    assert_eq!(cancelled.status, 204);
    assert_eq!(get(addr, &exec).await.status, 404);

    // DELETE the script -> 204; a subsequent GET is 404.
    let deleted = request(addr, "DELETE", &detail, None).await;
    assert_eq!(deleted.status, 204);
    assert_eq!(get(addr, &detail).await.status, 404);

    // Starting an execution under an unknown script is 404.
    let unknown = request(
        addr,
        "POST",
        "/api/v1/apps/gw/scripts/script-999/executions",
        None,
    )
    .await;
    assert_eq!(
        unknown.status, 404,
        "start of unknown script: {}",
        unknown.body
    );
}
