//! Integration tests for the bulk-data write family (`REQ_0972`).
//!
//! Drives a live axum server whose write seam is an in-memory
//! [`SimActionSink`] (no real effect) and exercises the full SOVD bulk-data
//! lifecycle over real TCP: upload raw bytes into a category (`POST` → `201`),
//! list the descriptor and the category, download the bytes back (round-trip),
//! delete one (`204`) and the `404` path.
//!
//! - `TEST_0947` — POST raw bytes → `201` with a descriptor (id + size); GET the
//!   category lists it; GET the categories shows count 1; GET the file → `200`
//!   with the bytes round-tripped; DELETE → `204` then GET is `404`.

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

/// Spawn a live server backed by a default (empty) write seam; bulk-data is
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

/// `TEST_0947` — the full bulk-data lifecycle over the wire.
#[tokio::test]
async fn bulk_data_lifecycle_over_http() {
    let addr = spawn().await;
    let base = "/api/v1/apps/gw/bulk-data";
    let category = format!("{base}/logs");

    // No categories and an empty category listing on a fresh resource.
    assert_eq!(
        json(&get(addr, base).await.body)["x-medkit"]["total_count"],
        0
    );
    assert_eq!(
        json(&get(addr, &category).await.body)["x-medkit"]["total_count"],
        0
    );

    // POST raw bytes -> 201 with a descriptor carrying an id + size.
    let payload = "hello bulk world";
    let uploaded = request(addr, "POST", &category, Some(payload)).await;
    assert_eq!(uploaded.status, 201, "upload: {}", uploaded.body);
    let desc = json(&uploaded.body);
    let file_id = desc["id"].as_str().expect("descriptor id").to_owned();
    assert!(file_id.starts_with("file-"));
    assert_eq!(desc["size"], payload.len());

    // GET the category lists the descriptor.
    let listed = json(&get(addr, &category).await.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], file_id.as_str());

    // GET the categories shows the category with count 1.
    let cats = json(&get(addr, base).await.body);
    assert_eq!(cats["x-medkit"]["total_count"], 1);
    assert_eq!(cats["items"][0]["id"], "logs");
    assert_eq!(cats["items"][0]["count"], 1);

    // GET the file -> 200 and the bytes round-trip.
    let file = format!("{category}/{file_id}");
    let downloaded = get(addr, &file).await;
    assert_eq!(downloaded.status, 200);
    assert_eq!(downloaded.body, payload);

    // DELETE the file -> 204; a subsequent GET is 404.
    let deleted = request(addr, "DELETE", &file, None).await;
    assert_eq!(deleted.status, 204);
    assert_eq!(get(addr, &file).await.status, 404);
}
