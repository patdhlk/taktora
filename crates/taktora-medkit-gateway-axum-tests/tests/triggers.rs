//! Integration tests for the triggers + SSE event-stream slice (#85).
//!
//! - `TEST_0919` — the `/triggers` subscription CRUD surface registers, lists,
//!   fetches, and removes a basic entity/severity trigger over real TCP.
//! - `TEST_0920` — the refresh-and-diff loop turns a provider whose snapshot
//!   changes between polls into `fault_raised` / `fault_cleared` SSE frames whose
//!   wire shape matches `contract/golden/faults_stream_sse_sample.txt`.
//! - `TEST_0921` — a health transition emits a `health_changed` SSE frame.
//!
//! The server binds `127.0.0.1:0` (ephemeral); a hand-rolled HTTP/1.1 client
//! keeps the suite dependency-light, mirroring `walking_skeleton.rs`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{
    GatewayConfig, demo, ephemeral_listener, serve_listener, serve_listener_with_provider,
};
use taktora_medkit_model::{Entity, EntityKind, EntityMeta, FaultSummary, Ros2Ref, Severity};
use taktora_medkit_provider::{Provider, ProviderSnapshot};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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

fn json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not JSON ({e}): {body}"))
}

/// Spawn the static (no refresh loop) server over the demo provider.
async fn spawn_static() -> SocketAddr {
    let gateway = Gateway::new(demo::provider());
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// `TEST_0919` — the triggers subscription CRUD surface works end to end.
// @need-ids: TEST_0919
#[tokio::test]
async fn triggers_crud_round_trip() {
    let addr = spawn_static().await;

    // POST registers a basic entity/severity subscription -> 201 with an id.
    let created = request(
        addr,
        "POST",
        "/api/v1/triggers",
        Some(r#"{"entity_id":"ros2_medkit_gateway","severity":2}"#),
    )
    .await;
    assert_eq!(
        created.status, 201,
        "POST /triggers should create: {}",
        created.body
    );
    let created = json(&created.body);
    let id = created["id"]
        .as_str()
        .expect("created trigger carries an id");
    assert_eq!(created["entity_id"], "ros2_medkit_gateway");
    assert_eq!(created["severity"], 2);

    // GET lists it.
    let listed = request(addr, "GET", "/api/v1/triggers", None).await;
    assert_eq!(listed.status, 200);
    let listed = json(&listed.body);
    let items = listed["items"].as_array().expect("items array");
    assert!(
        items.iter().any(|t| t["id"] == id),
        "list should contain {id}"
    );

    // GET {id} fetches it.
    let fetched = request(addr, "GET", &format!("/api/v1/triggers/{id}"), None).await;
    assert_eq!(fetched.status, 200);
    assert_eq!(json(&fetched.body)["id"], id);

    // DELETE {id} removes it; subsequent GET 404s.
    let deleted = request(addr, "DELETE", &format!("/api/v1/triggers/{id}"), None).await;
    assert_eq!(deleted.status, 204);
    let gone = request(addr, "GET", &format!("/api/v1/triggers/{id}"), None).await;
    assert_eq!(gone.status, 404);
}

// ---- A provider whose snapshot changes between polls -----------------------

/// Phases: 0 = healthy (no fault), 1 = fault raised, 2 = fault cleared again.
#[derive(Clone)]
struct PhasedProvider {
    phase: Arc<AtomicUsize>,
}

const ENTITY: &str = "ros2_medkit_gateway";

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
    fn health(&self, _entity_id: &str) -> taktora_medkit_model::Health {
        taktora_medkit_model::Health::Ok
    }
    fn snapshot(&self) -> ProviderSnapshot {
        let mut faults = std::collections::BTreeMap::new();
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

/// Parse SSE frames into `(event_type, data_json)` pairs.
fn sse_frames(text: &str) -> Vec<(String, Value)> {
    let body = text.split_once("\r\n\r\n").map_or(text, |(_, b)| b);
    let mut frames = Vec::new();
    for block in body.split("\n\n") {
        let mut event = None;
        let mut data = None;
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event = Some(rest.trim().to_owned());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data = serde_json::from_str::<Value>(rest.trim()).ok();
            }
        }
        if let (Some(e), Some(d)) = (event, data) {
            frames.push((e, d));
        }
    }
    frames
}

/// Open an SSE connection to `/api/v1/triggers/events`.
async fn open_sse(addr: SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let req = "GET /api/v1/triggers/events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n";
    stream.write_all(req.as_bytes()).await.expect("write");
    stream
}

/// `TEST_0920` — raising then clearing a fault yields `fault_raised` then
/// `fault_cleared` SSE frames whose data object matches the golden shape.
// @need-ids: TEST_0920
#[tokio::test]
async fn fault_raise_and_clear_stream_as_sse() {
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

    // Register a trigger for the entity so the stream delivers its events.
    let created = request(
        addr,
        "POST",
        "/api/v1/triggers",
        Some(r#"{"entity_id":"ros2_medkit_gateway"}"#),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.body);

    let stream = open_sse(addr).await;

    // Drive the provider through raise -> clear while the stream is open.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        phase.store(1, Ordering::SeqCst); // raise
        tokio::time::sleep(Duration::from_millis(120)).await;
        phase.store(2, Ordering::SeqCst); // clear
    });

    let text = read_sse(stream, Duration::from_millis(500)).await;
    let frames = sse_frames(&text);

    let raised = frames
        .iter()
        .find(|(e, _)| e == "fault_raised")
        .unwrap_or_else(|| panic!("expected a fault_raised frame in: {text}"));
    let cleared = frames
        .iter()
        .find(|(e, _)| e == "fault_cleared")
        .unwrap_or_else(|| panic!("expected a fault_cleared frame in: {text}"));

    // Golden frame shape: event_type, fault sub-object, timestamp, x-medkit.
    for (event_type, data) in [raised, cleared] {
        assert_eq!(data["event_type"], event_type.as_str());
        assert_eq!(data["fault"]["fault_code"], "BRAKE_PRESSURE_LOW");
        assert_eq!(data["fault"]["severity"], 2);
        assert!(data["timestamp"].is_number());
        assert_eq!(data["x-medkit"]["entity_id"], "ros2_medkit_gateway");
        assert_eq!(data["x-medkit"]["entity_type"], "apps");
    }
}

/// `TEST_0921` — a health transition (Ok -> Error) emits a `health_changed` frame.
// @need-ids: TEST_0921
#[tokio::test]
async fn health_transition_streams_health_changed() {
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

    // A trigger with no filter matches everything.
    let created = request(addr, "POST", "/api/v1/triggers", Some("{}")).await;
    assert_eq!(created.status, 201, "{}", created.body);

    let stream = open_sse(addr).await;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        phase.store(1, Ordering::SeqCst); // Ok -> Error
    });

    let text = read_sse(stream, Duration::from_millis(400)).await;
    let frames = sse_frames(&text);
    assert!(
        frames.iter().any(|(e, _)| e == "health_changed"),
        "expected a health_changed frame in: {text}"
    );
}
