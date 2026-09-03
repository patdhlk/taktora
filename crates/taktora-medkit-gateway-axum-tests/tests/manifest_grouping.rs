//! Manifest-grouping integration test: drive a **live** axum server whose
//! [`GatewayConfig`] carries a grouping manifest over a provider that emits only
//! flat, raw entities (`app:<task>`, `component:<subdevice>`), and assert the
//! declared Area/Component structure surfaces through the HTTP relationship
//! sub-resources.
//!
//! - `TEST_0910` — applying a manifest re-parents the raw provider entities so
//!   `GET /api/v1/areas/{id}/components` and the component-nesting sub-resources
//!   return the declared structure (`REQ_0920`, `REQ_0921`).
//! - `TEST_0911` — an absent manifest leaves the flat grouping intact, no panic
//!   (`REQ_0922`).
//!
//! The server binds `127.0.0.1:0` (ephemeral), so the suite is collision-free.

use std::net::SocketAddr;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, ephemeral_listener, router_from_gateway};
use taktora_medkit_manifest::Manifest;
use taktora_medkit_model::{Entity, EntityKind, EntityMeta, Ros2Ref};
use taktora_medkit_provider::MockProvider;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn raw_app(task: &str) -> Entity {
    Entity {
        href: format!("/api/v1/apps/app:{task}"),
        id: format!("app:{task}"),
        name: task.to_owned(),
        kind: EntityKind::App,
        parent_id: None,
        description: None,
        x_medkit: Some(EntityMeta {
            is_online: Some(true),
            ros2: Some(Ros2Ref {
                node: format!("/{task}"),
            }),
            source: Some("heuristic".to_owned()),
            ..EntityMeta::default()
        }),
    }
}

fn raw_subdevice(addr: &str) -> Entity {
    Entity {
        href: format!("/api/v1/components/component:{addr}"),
        id: format!("component:{addr}"),
        name: addr.to_owned(),
        kind: EntityKind::Component,
        parent_id: None,
        description: None,
        x_medkit: None,
    }
}

/// A provider emitting only flat raw entities — no Area/Component grouping.
fn raw_provider() -> MockProvider {
    MockProvider::new()
        .with_entity(raw_app("planner"))
        .with_entity(raw_app("controller"))
        .with_entity(raw_subdevice("0x01"))
}

fn manifest() -> Manifest {
    Manifest::builder()
        .area("drive", "Drive train")
        .component("nav", "drive", "Navigation")
        .map_task("planner", "nav")
        .map_task("controller", "nav")
        .map_subdevice("0x01", "nav")
        .build()
}

/// Spawn a live server whose `GatewayConfig` carries `manifest`, returning its
/// bound address.
async fn spawn(provider: MockProvider, manifest: Option<Manifest>) -> SocketAddr {
    let gateway = Gateway::new(provider);
    let config = GatewayConfig {
        manifest,
        ..GatewayConfig::default()
    };
    let app = router_from_gateway(&gateway, &config);
    let (listener, addr) = ephemeral_listener().await.expect("bind ephemeral port");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// Minimal HTTP/1.1 GET: one request per connection (`Connection: close`).
async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
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
    let value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("{path}: body is not JSON ({e}): {body}"));
    (status, value)
}

fn ids(collection: &Value) -> Vec<String> {
    collection["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["id"].as_str().expect("id").to_owned())
        .collect()
}

/// `TEST_0910` — a manifest re-parents the raw entities so the declared structure
/// surfaces through the HTTP relationship sub-resources.
// @need-ids: TEST_0910
#[tokio::test]
async fn manifest_groups_raw_entities() {
    let addr = spawn(raw_provider(), Some(manifest())).await;

    // The declared Area and Component appear in the top-level lists.
    let (status, areas) = get_json(addr, "/api/v1/areas").await;
    assert_eq!(status, 200);
    assert!(ids(&areas).contains(&"drive".to_owned()));

    let (status, components) = get_json(addr, "/api/v1/components").await;
    assert_eq!(status, 200);
    assert!(ids(&components).contains(&"nav".to_owned()));

    // `/areas/{id}/components` returns the declared component.
    let (status, grouped) = get_json(addr, "/api/v1/areas/drive/components").await;
    assert_eq!(status, 200);
    assert_eq!(ids(&grouped), vec!["nav".to_owned()]);
    assert_eq!(grouped["_links"]["self"], "/api/v1/areas/drive/components");

    // `/components/{id}/hosts` returns the re-parented apps.
    let (status, hosts) = get_json(addr, "/api/v1/components/nav/hosts").await;
    assert_eq!(status, 200);
    let host_ids = ids(&hosts);
    assert!(host_ids.contains(&"app:planner".to_owned()));
    assert!(host_ids.contains(&"app:controller".to_owned()));

    // `/components/{id}/subcomponents` returns the re-parented subdevice.
    let (status, subs) = get_json(addr, "/api/v1/components/nav/subcomponents").await;
    assert_eq!(status, 200);
    assert_eq!(ids(&subs), vec!["component:0x01".to_owned()]);
}

/// `TEST_0911` — without a manifest the grouping stays flat: no Areas, and the
/// component nesting sub-resources are empty rather than panicking.
// @need-ids: TEST_0911
#[tokio::test]
async fn absent_manifest_stays_flat() {
    let addr = spawn(raw_provider(), None).await;

    let (status, areas) = get_json(addr, "/api/v1/areas").await;
    assert_eq!(status, 200);
    assert!(ids(&areas).is_empty());

    // The raw subdevice component is present but groups nothing under it.
    let (status, hosts) = get_json(addr, "/api/v1/components/component:0x01/hosts").await;
    assert_eq!(status, 200);
    assert!(ids(&hosts).is_empty());
}
