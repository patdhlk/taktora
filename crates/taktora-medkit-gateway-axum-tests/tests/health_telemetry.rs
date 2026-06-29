//! Integration tests for provider-sourced `/health` telemetry (`REQ_0978`).
//!
//! Drives a live axum server over a [`MockProvider`] and asserts the served
//! `/health` document reflects provider-supplied telemetry when present, and
//! falls back to the best-effort zero baseline when absent — both over real TCP.
//!
//! - `TEST_0953` — a provider with telemetry surfaces the exact override values
//!   (and keeps the real entity-cache `apps` count); a plain provider keeps the
//!   zero-filled blocks alongside the real counts (back-compat).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::{Value, json};
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, ephemeral_listener, serve_listener};
use taktora_medkit_model::{Entity, EntityKind, EntityMeta, Ros2Ref};
use taktora_medkit_provider::{MockProvider, Telemetry};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn get(addr: SocketAddr, path: &str) -> Value {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let mut head = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n");
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (_, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not JSON ({e}): {body}"))
}

fn app(id: &str) -> Entity {
    Entity {
        href: format!("/api/v1/apps/{id}"),
        id: id.to_owned(),
        name: id.to_owned(),
        kind: EntityKind::App,
        parent_id: None,
        description: None,
        x_medkit: Some(EntityMeta {
            is_online: Some(true),
            ros2: Some(Ros2Ref {
                node: format!("/{id}"),
            }),
            source: Some("heuristic".to_owned()),
            ..EntityMeta::default()
        }),
    }
}

async fn serve(provider: MockProvider) -> SocketAddr {
    let view = Arc::new(Gateway::new(provider).view());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// `TEST_0953` — a server over a provider carrying telemetry surfaces the exact
/// override values, while the live entity-cache `apps` count stays authoritative.
#[tokio::test]
async fn health_surfaces_provider_telemetry() {
    let telemetry = Telemetry {
        data_provider: BTreeMap::from([("pool_cap".to_owned(), json!(256))]),
        subscription_executor: BTreeMap::from([("worker_alive".to_owned(), json!(true))]),
        entity_cache: BTreeMap::from([("generation".to_owned(), json!(7))]),
    };
    let provider = MockProvider::new()
        .with_entity(app("gw"))
        .with_telemetry(telemetry);
    let addr = serve(provider).await;

    let health = get(addr, "/api/v1/health").await;
    assert_eq!(health["x-medkit-data-provider"]["pool_cap"], 256);
    assert_eq!(
        health["x-medkit-subscription-executor"]["worker_alive"],
        true
    );
    assert_eq!(health["x-medkit-entity-cache"]["generation"], 7);
    // The real entity-cache count is still correct.
    assert_eq!(health["x-medkit-entity-cache"]["apps"], 1);
}

/// `TEST_0953` (back-compat) — a server over a plain provider with no telemetry
/// keeps the zero-filled blocks alongside the real counts.
#[tokio::test]
async fn health_without_telemetry_keeps_zero_defaults() {
    let addr = serve(MockProvider::new().with_entity(app("gw"))).await;

    let health = get(addr, "/api/v1/health").await;
    assert_eq!(health["x-medkit-data-provider"]["pool_cap"], 0);
    assert_eq!(health["x-medkit-subscription-executor"]["queue_depth"], 0);
    assert_eq!(health["x-medkit-entity-cache"]["generation"], 0);
    // The real entity-cache count is still correct.
    assert_eq!(health["x-medkit-entity-cache"]["apps"], 1);
}
