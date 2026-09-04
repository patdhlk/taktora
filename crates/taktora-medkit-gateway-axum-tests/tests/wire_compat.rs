//! Wire-compatibility integration tests for the Tier-A `ros2_medkit` parity pass
//! (`REQ_0961`–`REQ_0968`): the fixes that close gaps *inside* the already-served
//! surface so a path/field-hardcoding upstream client is not surprised.
//!
//! - `TEST_0936` (`REQ_0965`) — the root capability flags and endpoint catalogue
//!   honestly advertise the served vendor extensions.
//! - `TEST_0937` (`REQ_0967`) — `/health` carries the golden's `x-medkit-*`
//!   telemetry blocks and a `timestamp`.
//! - `TEST_0938` (`REQ_0964`) — global `DELETE /faults` acknowledges with `204`.
//! - `TEST_0939` (`REQ_0968`) — auth on → `/auth/token` `200`; auth off → `404`
//!   (absent), never the `501` deferred fallback.
//! - `TEST_0940` (`REQ_0963`) — locks expose `GET` list + detail with the
//!   `owned` flag keyed off `X-Client-Id`.
//! - `TEST_0941` (`REQ_0962`) — triggers are reachable per entity, pinned to the
//!   path entity, and never leak across entities.
//! - `TEST_0942` (`REQ_0961`, `REQ_0966`) — the global `/faults/stream` replays
//!   the retained ring on a fresh connect and honours `Last-Event-ID`.
//!
//! The server binds `127.0.0.1:0` (ephemeral); a hand-rolled HTTP/1.1 client
//! keeps the suite dependency-light, mirroring the sibling test files.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{
    GatewayConfig, demo, ephemeral_listener, serve_listener, serve_listener_with_provider,
};
use taktora_medkit_model::{
    Entity, EntityKind, EntityMeta, FaultSummary, Health, Ros2Ref, Severity,
};
use taktora_medkit_provider::{Provider, ProviderSnapshot};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ---- Minimal HTTP/1.1 client (one request per connection) ------------------

struct Response {
    status: u16,
    body: String,
}

/// One request per connection with arbitrary headers and an optional JSON body.
async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Response {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
    for (k, v) in headers {
        let _ = write!(head, "{k}: {v}\r\n");
    }
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
    request(addr, "GET", path, &[], None).await
}

fn json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not JSON ({e}): {body}"))
}

/// Spawn the static (no refresh loop) server over the demo provider with `config`.
async fn spawn_static(config: GatewayConfig) -> SocketAddr {
    let gateway = Gateway::new(demo::provider());
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &config).await;
    });
    addr
}

/// `TEST_0936` (`REQ_0965`) — the root advertises the served extensions honestly.
// @need-ids: TEST_0936
#[tokio::test]
async fn root_capabilities_are_honest() {
    let addr = spawn_static(GatewayConfig::default()).await;
    let root = json(&get(addr, "/api/v1/").await.body);

    let caps = &root["capabilities"];
    // Served families flip to true; deferred families stay false.
    assert_eq!(caps["authentication"], true);
    assert_eq!(caps["locking"], true);
    assert_eq!(caps["triggers"], true);
    assert_eq!(caps["vendor_extensions"], true);
    assert_eq!(caps["operations"], true);
    // Bulk-data is a served family now (`REQ_0972`).
    assert_eq!(caps["bulk_data"], true);

    let endpoints: Vec<&str> = root["endpoints"]
        .as_array()
        .expect("endpoints array")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for expected in [
        "GET /api/v1/faults/stream",
        "DELETE /api/v1/faults",
        "GET /api/v1/triggers",
        "POST /api/v1/apps/{id}/triggers",
        "GET /api/v1/apps/{id}/locks",
        "POST /api/v1/auth/token",
    ] {
        assert!(
            endpoints.contains(&expected),
            "catalogue should advertise {expected}: {endpoints:?}"
        );
    }
}

/// `TEST_0937` (`REQ_0967`) — `/health` carries the golden telemetry blocks.
// @need-ids: TEST_0937
#[tokio::test]
async fn health_carries_telemetry_blocks() {
    let addr = spawn_static(GatewayConfig::default()).await;
    let health = json(&get(addr, "/api/v1/health").await.body);

    assert_eq!(health["status"], "healthy");
    assert!(health["timestamp"].is_number(), "edge-stamped timestamp");
    // The entity-cache counts are real; the demo provider has apps.
    assert!(health["x-medkit-entity-cache"]["apps"].is_number());
    assert!(health["x-medkit-entity-cache"]["capacity"].is_number());
    // Best-effort placeholder blocks are present and field-complete.
    assert!(health["x-medkit-data-provider"]["pool_cap"].is_number());
    assert_eq!(
        health["x-medkit-subscription-executor"]["worker_alive"],
        true
    );
    assert_eq!(health["x-medkit-subscription-executor"]["degraded"], false);
}

/// `TEST_0938` (`REQ_0964`) — global `DELETE /faults` answers `204`.
// @need-ids: TEST_0938
#[tokio::test]
async fn global_delete_faults_acknowledges() {
    let addr = spawn_static(GatewayConfig::default()).await;
    let deleted = request(addr, "DELETE", "/api/v1/faults", &[], None).await;
    assert_eq!(
        deleted.status, 204,
        "clear-all should ack: {}",
        deleted.body
    );
}

/// `TEST_0939` (`REQ_0968`) — auth on issues a token; auth off answers `404`.
// @need-ids: TEST_0939
#[tokio::test]
async fn auth_disabled_is_404_not_501() {
    let creds = r#"{"grant_type":"client_credentials"}"#;

    // Default config: auth enabled -> 200 token.
    let on = spawn_static(GatewayConfig::default()).await;
    let issued = request(on, "POST", "/api/v1/auth/token", &[], Some(creds)).await;
    assert_eq!(
        issued.status, 200,
        "auth on issues a token: {}",
        issued.body
    );

    // Auth disabled -> the family is absent (404), not deferred (501).
    let off = spawn_static(GatewayConfig {
        auth_enabled: false,
        ..GatewayConfig::default()
    })
    .await;
    let refused = request(off, "POST", "/api/v1/auth/token", &[], Some(creds)).await;
    assert_eq!(refused.status, 404, "auth off is absent: {}", refused.body);
    assert_eq!(json(&refused.body)["error_code"], "not-found");
}

/// `TEST_0940` (`REQ_0963`) — locks expose `GET` list + detail with `owned`.
// @need-ids: TEST_0940
#[tokio::test]
async fn lock_reads_expose_owner_view() {
    let addr = spawn_static(GatewayConfig::default()).await;
    let resource = "/api/v1/apps/ros2_medkit_gateway/locks";

    // Acquire as alice.
    let acquired = request(
        addr,
        "POST",
        resource,
        &[("X-Client-Id", "alice")],
        Some(r#"{"lock_expiration":60000}"#),
    )
    .await;
    assert_eq!(acquired.status, 201, "acquire: {}", acquired.body);
    let lock_id = json(&acquired.body)["id"].as_str().unwrap().to_owned();

    // GET list as alice -> her lock, owned: true.
    let listed = request(addr, "GET", resource, &[("X-Client-Id", "alice")], None).await;
    assert_eq!(listed.status, 200);
    let listed = json(&listed.body);
    assert_eq!(listed["x-medkit"]["total_count"], 1);
    assert_eq!(listed["items"][0]["id"], lock_id.as_str());
    assert_eq!(listed["items"][0]["owned"], true);

    // GET detail without a client id -> found, but owned: false.
    let detail = request(addr, "GET", &format!("{resource}/{lock_id}"), &[], None).await;
    assert_eq!(detail.status, 200);
    assert_eq!(json(&detail.body)["owned"], false);

    // GET a non-existent lock -> 404.
    let missing = get(addr, &format!("{resource}/lck-does-not-exist")).await;
    assert_eq!(missing.status, 404);
}

/// `TEST_0941` (`REQ_0962`) — triggers are reachable per entity and entity-scoped.
// @need-ids: TEST_0941
#[tokio::test]
async fn entity_scoped_triggers_are_pinned() {
    let addr = spawn_static(GatewayConfig::default()).await;
    let app = "/api/v1/apps/ros2_medkit_gateway/triggers";

    // POST under the app pins entity_id to the path entity, ignoring the body.
    let created = request(addr, "POST", app, &[], Some(r#"{"severity":2}"#)).await;
    assert_eq!(created.status, 201, "{}", created.body);
    let created = json(&created.body);
    let id = created["id"].as_str().unwrap().to_owned();
    assert_eq!(created["entity_id"], "ros2_medkit_gateway");

    // The entity-scoped list shows it.
    let listed = json(&get(addr, app).await.body);
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == id.as_str()),
        "entity list should contain {id}"
    );

    // A different entity never sees it -> 404 on the cross-entity fetch.
    let cross = get(
        addr,
        &format!("/api/v1/components/spark-6723/triggers/{id}"),
    )
    .await;
    assert_eq!(cross.status, 404, "triggers must not leak across entities");

    // The owning entity fetches and deletes it.
    let fetched = get(addr, &format!("{app}/{id}")).await;
    assert_eq!(fetched.status, 200);
    let deleted = request(addr, "DELETE", &format!("{app}/{id}"), &[], None).await;
    assert_eq!(deleted.status, 204);
}

// ---- A provider whose snapshot changes between polls, for the stream test ---

const ENTITY: &str = "ros2_medkit_gateway";

#[derive(Clone)]
struct PhasedProvider {
    phase: Arc<AtomicUsize>,
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
            ros2: Some(Ros2Ref {
                node: format!("/{ENTITY}"),
            }),
            ..EntityMeta::default()
        }),
    }
}

fn brake_fault() -> FaultSummary {
    FaultSummary {
        description: "Brake circuit pressure below safe threshold".to_owned(),
        fault_code: "BRAKE_PRESSURE_LOW".to_owned(),
        first_occurred: 1_782_600_000.25,
        last_occurred: 1_782_661_500.75,
        occurrence_count: 7,
        reporting_sources: vec![format!("/{ENTITY}")],
        severity: Severity::Error.wire_value(),
        severity_label: "ERROR".to_owned(),
        status: "CONFIRMED".to_owned(),
    }
}

impl Provider for PhasedProvider {
    fn entities(&self) -> Vec<Entity> {
        vec![app_entity()]
    }
    fn faults(&self, entity_id: &str) -> Vec<FaultSummary> {
        if entity_id == ENTITY && self.phase.load(Ordering::SeqCst) == 1 {
            vec![brake_fault()]
        } else {
            Vec::new()
        }
    }
    fn health(&self, _entity_id: &str) -> Health {
        Health::Ok
    }
    fn snapshot(&self) -> ProviderSnapshot {
        let mut faults = BTreeMap::new();
        let entity_faults = self.faults(ENTITY);
        if !entity_faults.is_empty() {
            faults.insert(ENTITY.to_owned(), entity_faults);
        }
        ProviderSnapshot {
            entities: self.entities(),
            faults,
            ..ProviderSnapshot::default()
        }
    }
}

/// Open an SSE connection to `path`, optionally with a `Last-Event-ID`.
async fn open_sse(addr: SocketAddr, path: &str, last_event_id: Option<&str>) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let leid = last_event_id.map_or_else(String::new, |v| format!("Last-Event-ID: {v}\r\n"));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n{leid}\r\n"
    );
    stream.write_all(req.as_bytes()).await.expect("write");
    stream
}

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

/// `TEST_0942` (`REQ_0961`, `REQ_0966`) — the global `/faults/stream` is
/// unfiltered (no trigger needed), replays the retained ring on a fresh connect,
/// and honours `Last-Event-ID`.
// @need-ids: TEST_0942
#[tokio::test]
async fn global_stream_replays_ring_and_honours_last_event_id() {
    let phase = Arc::new(AtomicUsize::new(0));
    let provider = PhasedProvider {
        phase: Arc::clone(&phase),
    };
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener_with_provider(
            listener,
            provider,
            &GatewayConfig::default(),
            Duration::from_millis(25),
        )
        .await;
    });

    // Let the loop establish a healthy baseline, then raise the fault BEFORE any
    // client connects so the 0->1 transition is diffed and retained in the ring.
    tokio::time::sleep(Duration::from_millis(80)).await;
    phase.store(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(120)).await;

    // A fresh connect to the GLOBAL stream replays the ring — no trigger needed.
    let text = read_sse(
        open_sse(addr, "/api/v1/faults/stream", None).await,
        Duration::from_millis(250),
    )
    .await;
    assert!(
        text.contains("event: fault_raised") && text.contains("BRAKE_PRESSURE_LOW"),
        "fresh connect should replay the retained fault_raised: {text}"
    );

    // Reconnect with a Last-Event-ID past every retained id: nothing is replayed.
    let text = read_sse(
        open_sse(addr, "/api/v1/faults/stream", Some("1000000")).await,
        Duration::from_millis(150),
    )
    .await;
    assert!(
        !text.contains("event: fault_raised"),
        "Last-Event-ID past the ring must suppress replay: {text}"
    );
}
