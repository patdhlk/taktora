//! Lock integration tests: drive a **live** axum server and exercise the SOVD
//! diagnostic-scoped exclusive-access surface over real TCP (issue #149).
//!
//! - `TEST_0925` — acquire → extend → release round-trip with contract-shaped
//!   bodies and the required `X-Client-Id` holder header.
//! - `TEST_0926` — a second client acquiring a held lock without `break_lock`
//!   gets `409`; with `break_lock` the incumbent is evicted.
//! - `TEST_0927` — ownership is enforced by `X-Client-Id`; a missing header is
//!   `400`; the registry is in-memory and off the control path.
//!
//! Deterministic TTL expiry is unit-tested in the registry (`REQ_0941`); these
//! live-server tests never sleep on real time.

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

struct Response {
    status: u16,
    body: String,
}

/// Minimal HTTP/1.1 client with an optional `X-Client-Id` header and JSON body;
/// one request per connection (`Connection: close`).
async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    client_id: Option<&str>,
    body: Option<&str>,
) -> Response {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let id_header = client_id.map_or_else(String::new, |c| format!("X-Client-Id: {c}\r\n"));
    let body_part = body.map_or_else(
        || "\r\n".to_owned(),
        |b| {
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
                b.len()
            )
        },
    );
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{id_header}Connection: close\r\n{body_part}"
    );
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

fn json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not JSON ({e}): {body}"))
}

const LOCKS: &str = "/api/v1/components/sensor/locks";

/// `TEST_0925` — acquire (`201`, contract-shaped `Lock`) → extend (`204`) →
/// release (`204`) round-trip with the `X-Client-Id` holder header.
#[tokio::test]
async fn acquire_extend_release_round_trip() {
    let addr = spawn().await;

    let acquire = request(
        addr,
        "POST",
        LOCKS,
        Some("alice"),
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(acquire.status, 201, "acquire must be 201: {}", acquire.body);
    let lock = json(&acquire.body);
    assert_eq!(lock["owned"], true);
    let lock_id = lock["id"].as_str().expect("lock id").to_owned();
    let expiry = lock["lock_expiration"].as_str().expect("expiration");
    assert!(
        expiry.contains('T') && expiry.ends_with('Z'),
        "lock_expiration must be RFC3339 absolute: {expiry}"
    );

    let item = format!("{LOCKS}/{lock_id}");
    let extend = request(
        addr,
        "PUT",
        &item,
        Some("alice"),
        Some(r#"{"lock_expiration":120000}"#),
    )
    .await;
    assert_eq!(extend.status, 204, "extend must be 204: {}", extend.body);

    let release = request(addr, "DELETE", &item, Some("alice"), None).await;
    assert_eq!(release.status, 204, "release must be 204: {}", release.body);

    // After release the resource is free: a fresh acquire succeeds.
    let reacquire = request(
        addr,
        "POST",
        LOCKS,
        Some("bob"),
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(
        reacquire.status, 201,
        "re-acquire after release must be 201"
    );
}

/// `TEST_0926` — a second client without `break_lock` gets `409`; with
/// `break_lock` the held lock is evicted and the supervisor acquires.
#[tokio::test]
async fn conflict_then_break_lock_override() {
    let addr = spawn().await;

    let first = request(
        addr,
        "POST",
        LOCKS,
        Some("alice"),
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(first.status, 201);

    let conflict = request(
        addr,
        "POST",
        LOCKS,
        Some("bob"),
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(
        conflict.status, 409,
        "second client without break_lock must be 409: {}",
        conflict.body
    );

    let broken = request(
        addr,
        "POST",
        LOCKS,
        Some("bob"),
        Some(r#"{"lock_expiration":60000,"break_lock":true}"#),
    )
    .await;
    assert_eq!(
        broken.status, 201,
        "break_lock must evict and acquire: {}",
        broken.body
    );
    assert_eq!(json(&broken.body)["owned"], true);
}

/// `TEST_0927` — ownership is enforced by `X-Client-Id`: a non-owner cannot
/// extend (`409`); a missing header is `400`.
#[tokio::test]
async fn ownership_and_client_id_enforced() {
    let addr = spawn().await;

    // Missing X-Client-Id → 400.
    let no_id = request(
        addr,
        "POST",
        LOCKS,
        None,
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(no_id.status, 400, "missing X-Client-Id must be 400");

    let acquire = request(
        addr,
        "POST",
        LOCKS,
        Some("alice"),
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(acquire.status, 201);
    let lock_id = json(&acquire.body)["id"].as_str().expect("id").to_owned();
    let item = format!("{LOCKS}/{lock_id}");

    // A non-owner cannot extend the live lock.
    let steal = request(
        addr,
        "PUT",
        &item,
        Some("bob"),
        Some(r#"{"lock_expiration":120000}"#),
    )
    .await;
    assert_eq!(
        steal.status, 409,
        "non-owner extend must be 409: {}",
        steal.body
    );
}
