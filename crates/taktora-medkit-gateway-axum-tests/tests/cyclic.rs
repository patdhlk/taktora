//! Integration tests for the cyclic-subscriptions slice (`REQ_0977`).
//!
//! - `TEST_0952` — the `…/{id}/cyclic-subscriptions` CRUD surface registers,
//!   fetches, lists, updates, and removes a periodic data-sampling subscription
//!   pinned to its entity, and refuses cross-entity access with a `404`; the
//!   per-subscription `…/events` SSE stream samples the entity's data on its
//!   cadence and pushes each sample as an `event: sample` frame.
//!
//! The server binds `127.0.0.1:0` (ephemeral); a hand-rolled HTTP/1.1 client
//! keeps the suite dependency-light, mirroring `triggers.rs`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, ephemeral_listener, serve_listener};
use taktora_medkit_model::{Entity, EntityKind, EntityMeta, Ros2Ref};
use taktora_medkit_provider::MockProvider;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ENTITY: &str = "ros2_medkit_gateway";

// ---- Minimal HTTP/1.1 client (one request per connection) ------------------

struct Response {
    status: u16,
    body: String,
}

async fn request(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> Response {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let payload = body.map_or_else(String::new, |b| {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        )
    });
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{payload}Connection: close\r\n\r\n{}",
        body.unwrap_or("")
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

fn json_body(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not JSON ({e}): {body}"))
}

fn app_entity() -> Entity {
    Entity {
        href: format!("/api/v1/apps/{ENTITY}"),
        id: ENTITY.to_owned(),
        name: ENTITY.to_owned(),
        kind: EntityKind::App,
        parent_id: None,
        description: None,
        x_medkit: Some(EntityMeta {
            is_online: Some(true),
            ros2: Some(Ros2Ref {
                node: format!("/{ENTITY}"),
            }),
            source: Some("runtime".to_owned()),
            ..EntityMeta::default()
        }),
    }
}

/// Spawn the static server over a provider whose `gw` entity carries some data.
async fn spawn() -> SocketAddr {
    let provider = MockProvider::new()
        .with_entity(app_entity())
        .with_data(ENTITY, json!({ "temperature": 42, "cpu": { "load": 0.4 } }));
    let gateway = Gateway::new(provider);
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// `TEST_0952` — the cyclic-subscription CRUD surface works end to end and pins
/// the subscription to its entity (cross-entity access is a `404`).
#[tokio::test]
async fn cyclic_subscriptions_crud_round_trip() {
    let addr = spawn().await;
    let base = format!("/api/v1/apps/{ENTITY}/cyclic-subscriptions");

    // POST registers a subscription -> 201 with an id pinned to the entity.
    let created = request(
        addr,
        "POST",
        &base,
        Some(r#"{"data_path":"temperature","interval_ms":30}"#),
    )
    .await;
    assert_eq!(created.status, 201, "POST should create: {}", created.body);
    let created = json_body(&created.body);
    let id = created["id"].as_str().expect("created subscription id");
    assert_eq!(created["entity_id"], ENTITY);
    assert_eq!(created["interval_ms"], 30);
    assert_eq!(created["data_path"], "temperature");

    // GET {id} fetches it; the list shows it.
    let fetched = request(addr, "GET", &format!("{base}/{id}"), None).await;
    assert_eq!(fetched.status, 200);
    assert_eq!(json_body(&fetched.body)["id"], id);

    let listed = request(addr, "GET", &base, None).await;
    assert_eq!(listed.status, 200);
    let items = json_body(&listed.body)["items"]
        .as_array()
        .expect("items array")
        .clone();
    assert!(
        items.iter().any(|s| s["id"] == id),
        "list should contain {id}"
    );

    // PUT updates the spec -> 200 with the new interval, entity preserved.
    let updated = request(
        addr,
        "PUT",
        &format!("{base}/{id}"),
        Some(r#"{"interval_ms":50}"#),
    )
    .await;
    assert_eq!(updated.status, 200, "PUT should update: {}", updated.body);
    let updated = json_body(&updated.body);
    assert_eq!(updated["interval_ms"], 50);
    assert_eq!(updated["entity_id"], ENTITY);

    // A subscription fetched under a different entity is a 404.
    let cross = request(
        addr,
        "GET",
        &format!("/api/v1/components/other/cyclic-subscriptions/{id}"),
        None,
    )
    .await;
    assert_eq!(cross.status, 404, "cross-entity GET should 404");

    // DELETE removes it; subsequent GET 404s.
    let deleted = request(addr, "DELETE", &format!("{base}/{id}"), None).await;
    assert_eq!(deleted.status, 204);
    let gone = request(addr, "GET", &format!("{base}/{id}"), None).await;
    assert_eq!(gone.status, 404);
}

/// Read from an open SSE connection until `deadline`, returning the accumulated
/// text. The connection is dropped on return.
async fn read_sse(mut stream: TcpStream, deadline: Duration) -> String {
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(deadline, async {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
    })
    .await;
    String::from_utf8_lossy(&buf).into_owned()
}

/// Collect the `data:` payloads from an SSE body as parsed JSON.
fn sse_data_frames(text: &str) -> Vec<Value> {
    let body = text.split_once("\r\n\r\n").map_or(text, |(_, b)| b);
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|rest| serde_json::from_str::<Value>(rest.trim()).ok())
        .collect()
}

/// `TEST_0952` — the per-subscription SSE stream samples the entity's data on its
/// cadence and pushes each sample as a `data:` frame.
#[tokio::test]
async fn cyclic_subscription_samples_data_over_sse() {
    let addr = spawn().await;
    let base = format!("/api/v1/apps/{ENTITY}/cyclic-subscriptions");

    // A fast subscription scoped to the `temperature` subtree.
    let created = request(
        addr,
        "POST",
        &base,
        Some(r#"{"data_path":"temperature","interval_ms":30}"#),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.body);
    let id = json_body(&created.body)["id"]
        .as_str()
        .expect("subscription id")
        .to_owned();

    // Open the SSE stream and read for a short window.
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let req = format!(
        "GET {base}/{id}/events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await.expect("write");

    let text = read_sse(stream, Duration::from_millis(300)).await;
    let frames = sse_data_frames(&text);
    assert!(
        frames.iter().any(|d| *d == json!(42)),
        "expected at least one sampled `temperature` frame (42) in: {text}"
    );
}
