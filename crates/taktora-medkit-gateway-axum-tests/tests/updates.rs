//! Integration tests for the updates (software-update) write family (`REQ_0974`).
//!
//! Drives a live axum server whose write seam is an in-memory
//! [`SimActionSink`] (no real effect) and exercises the full SOVD update
//! lifecycle over real TCP. Updates are a **global** family, so every path is
//! top-level (`/api/v1/updates…`), not entity-scoped: register (`POST` → `201`),
//! read it back, list it, read its status, drive `prepare`/`execute` (`202`),
//! delete (`204`), and the `404` paths.
//!
//! - `TEST_0949` — POST registers → `201` with id + status `registered`; GET it
//!   → `200`; the list shows it; `…/status` → `{"status":"registered"}`; PUT
//!   `…/prepare` → `202` then `prepared`; PUT `…/execute` → `202` then
//!   `executed`; DELETE → `204` then GET is `404`; a transition on an unknown id
//!   is `404`.

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

/// Spawn a live server backed by a default (empty) write seam; updates are
/// registered at runtime, so no pre-configuration is needed.
async fn spawn() -> SocketAddr {
    let view = Arc::new(Gateway::new(demo::provider()).view());
    let sink: Arc<dyn taktora_medkit_provider::ActionSink> = Arc::new(SimActionSink::new());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener_with_actions(listener, view, &GatewayConfig::default(), sink).await;
    });
    addr
}

/// `TEST_0949` — the full software-update lifecycle over the wire (global paths).
// @need-ids: TEST_0949
#[tokio::test]
async fn update_lifecycle_over_http() {
    let addr = spawn().await;
    let base = "/api/v1/updates";

    // The list starts empty.
    let empty = json(&get(addr, base).await.body);
    assert_eq!(empty["x-medkit"]["total_count"], 0);

    // POST registers -> 201 with an id and the `registered` status.
    let registered = request(addr, "POST", base, Some(r#"{"package":"fw-1.2.3"}"#)).await;
    assert_eq!(registered.status, 201, "register: {}", registered.body);
    let record = json(&registered.body);
    let id = record["id"].as_str().expect("id").to_owned();
    assert!(id.starts_with("update-"), "id: {id}");
    assert_eq!(record["status"], "registered");

    let one = format!("{base}/{id}");

    // GET reads it back; the list shows it.
    let got = get(addr, &one).await;
    assert_eq!(got.status, 200);
    assert_eq!(json(&got.body)["status"], "registered");
    let listed = json(&get(addr, base).await.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], id.as_str());

    // GET …/status reports the current state.
    let status = get(addr, &format!("{one}/status")).await;
    assert_eq!(status.status, 200);
    assert_eq!(json(&status.body)["status"], "registered");

    // PUT …/prepare -> 202; the status is now `prepared`.
    let prepared = request(addr, "PUT", &format!("{one}/prepare"), None).await;
    assert_eq!(prepared.status, 202, "prepare: {}", prepared.body);
    assert_eq!(json(&prepared.body)["status"], "prepared");
    assert_eq!(json(&get(addr, &one).await.body)["status"], "prepared");

    // PUT …/execute -> 202; the status is now `executed`.
    let executed = request(addr, "PUT", &format!("{one}/execute"), None).await;
    assert_eq!(executed.status, 202, "execute: {}", executed.body);
    assert_eq!(json(&executed.body)["status"], "executed");
    assert_eq!(
        json(&get(addr, &format!("{one}/status")).await.body)["status"],
        "executed"
    );

    // DELETE -> 204; a subsequent GET is 404.
    let deleted = request(addr, "DELETE", &one, None).await;
    assert_eq!(deleted.status, 204);
    assert_eq!(get(addr, &one).await.status, 404);

    // A transition on an unknown id is 404.
    let unknown = request(addr, "PUT", &format!("{base}/update-404/prepare"), None).await;
    assert_eq!(unknown.status, 404);
    assert_eq!(json(&unknown.body)["error_code"], "not-found");
}
