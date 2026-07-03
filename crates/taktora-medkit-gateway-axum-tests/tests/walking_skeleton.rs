//! Walking-skeleton integration tests: drive a **live** axum server over the
//! mock provider and assert the wire behaviour against the captured contract.
//!
//! - `TEST_0906` — every read-core response shape-matches its
//!   `contract/golden/*.json` fixture.
//! - `TEST_0907` — each deferred family answers `501` (not `404`) with a
//!   contract-shaped body.
//! - `TEST_0908` — the transport-hardening layers (CORS, rate limit) behave.
//!
//! The server binds `127.0.0.1:0` (ephemeral) and the test reads back the real
//! port, so the suite is parallel-safe and collision-free even under CI's
//! serial run.

use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{
    CorsConfig, GatewayConfig, RateLimit, demo, ephemeral_listener, serve_listener,
};
use taktora_medkit_model::BuildInfo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Spawn a live server with `config`, returning its bound address.
async fn spawn(config: GatewayConfig) -> SocketAddr {
    let gateway = Gateway::new(demo::provider());
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind ephemeral port");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &config).await;
    });
    addr
}

struct Response {
    status: u16,
    headers: String,
    body: String,
}

/// Minimal HTTP/1.1 client: one request per connection (`Connection: close`),
/// so the body is everything up to EOF. Avoids a heavyweight HTTP-client dep.
async fn request_with_origin(
    addr: SocketAddr,
    method: &str,
    path: &str,
    origin: Option<&str>,
) -> Response {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let origin_header = origin.map_or_else(String::new, |o| format!("Origin: {o}\r\n"));
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{origin_header}Connection: close\r\n\r\n"
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
        headers: head.to_lowercase(),
        body: body.to_owned(),
    }
}

async fn get(addr: SocketAddr, path: &str) -> Response {
    request_with_origin(addr, "GET", path, None).await
}

fn golden(name: &str) -> Value {
    let path = format!(
        "{}/../../contract/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read golden {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse golden {name}: {e}"))
}

/// Structural shape compatibility: every key/element structure the golden
/// constrains must be present and type-compatible in `actual`. Scalar *values*
/// may differ (the live skeleton is self-consistent, not a byte replay of the
/// mutually-inconsistent capture); `actual` may carry extra keys.
fn assert_shape(actual: &Value, golden: &Value, path: &str) {
    match (actual, golden) {
        (Value::Object(a), Value::Object(g)) => {
            for (key, gv) in g {
                let av = a
                    .get(key)
                    .unwrap_or_else(|| panic!("{path}: missing contract key `{key}`"));
                assert_shape(av, gv, &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(g)) => {
            // An empty golden array constrains nothing; otherwise every actual
            // element must match the golden's element template. An empty actual
            // array (fewer items in the self-consistent demo) is allowed.
            if let Some(template) = g.first() {
                for (i, av) in a.iter().enumerate() {
                    assert_shape(av, template, &format!("{path}[{i}]"));
                }
            }
        }
        (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::Null, Value::Null) => {}
        _ => panic!("{path}: type mismatch — actual {actual:?} vs contract {golden:?}"),
    }
}

async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let resp = get(addr, path).await;
    let value = serde_json::from_str(&resp.body)
        .unwrap_or_else(|e| panic!("{path}: body is not JSON ({e}): {}", resp.body));
    (resp.status, value)
}

/// `TEST_0906` — read-core families shape-match the captured contract corpus.
#[tokio::test]
async fn read_core_matches_golden_shapes() {
    let addr = spawn(GatewayConfig::default()).await;

    // Each (live path, golden fixture) pair must shape-match, 200.
    let cases: &[(&str, &str)] = &[
        ("/api/v1/", "root.json"),
        ("/api/v1/version-info", "version-info.json"),
        ("/api/v1/areas", "areas_list.json"),
        ("/api/v1/components", "components_list.json"),
        ("/api/v1/apps", "apps_list.json"),
        ("/api/v1/functions", "functions_list.json"),
        (
            "/api/v1/components/spark-6723/hosts",
            "component_hosts.json",
        ),
        (
            "/api/v1/components/spark-6723/depends-on",
            "component_depends-on.json",
        ),
        (
            "/api/v1/components/spark-6723/subcomponents",
            "component_subcomponents.json",
        ),
        ("/api/v1/functions/root/hosts", "function_hosts.json"),
        (
            "/api/v1/apps/ros2_medkit_gateway/is-located-on",
            "app_is-located-on.json",
        ),
        (
            "/api/v1/apps/ros2_medkit_gateway/belongs-to",
            "app_belongs-to.json",
        ),
        (
            "/api/v1/apps/ros2_medkit_gateway/depends-on",
            "app_depends-on.json",
        ),
        ("/api/v1/faults", "faults_list.json"),
        ("/api/v1/faults?status=all", "faults_list_all.json"),
        (
            "/api/v1/faults?status=pending",
            "faults_filtered_pending.json",
        ),
        (
            "/api/v1/components/spark-6723/faults",
            "component_faults_list.json",
        ),
        (
            "/api/v1/apps/ros2_medkit_gateway/faults",
            "app_faults_list.json",
        ),
        (
            "/api/v1/components/spark-6723/faults/BRAKE_PRESSURE_LOW",
            "fault_get_with_freezeframe.json",
        ),
    ];

    for (path, fixture) in cases {
        let (status, value) = get_json(addr, path).await;
        assert_eq!(status, 200, "{path} should be 200");
        assert_shape(&value, &golden(fixture), path);
    }
}

/// `TEST_0906` — an unknown entity yields the contract's `GenericError` 404.
#[tokio::test]
async fn unknown_entity_is_contract_404() {
    let addr = spawn(GatewayConfig::default()).await;
    let (status, value) = get_json(addr, "/api/v1/apps/does-not-exist").await;
    assert_eq!(status, 404);
    assert_shape(&value, &golden("error_not_found.json"), "error");
    assert_eq!(value["error_code"], "entity-not-found");
}

/// `TEST_0906` — `data` reads navigate the topic path; DELETE on a fault is 204.
#[tokio::test]
async fn data_and_delete_behaviour() {
    let addr = spawn(GatewayConfig::default()).await;

    let (status, value) = get_json(addr, "/api/v1/components/spark-6723/data").await;
    assert_eq!(status, 200);
    assert!(value.is_object());

    let (status, value) = get_json(addr, "/api/v1/components/spark-6723/data/cpu/load_avg").await;
    assert_eq!(status, 200);
    assert!(value.is_number());

    let resp = request_with_origin(
        addr,
        "DELETE",
        "/api/v1/components/spark-6723/faults/BRAKE_PRESSURE_LOW",
        None,
    )
    .await;
    assert_eq!(resp.status, 204);

    // Detail views are server-rendered best-effort: assert the core identity.
    let (status, value) = get_json(addr, "/api/v1/components/spark-6723").await;
    assert_eq!(status, 200);
    assert_eq!(value["id"], "spark-6723");
    assert_eq!(value["type"], "component");
    assert!(value["_links"]["self"].is_string());
}

/// `TEST_0907` — every deferred family answers `501` (not `404`) with a
/// contract-shaped body. At least one route per family is asserted.
#[tokio::test]
async fn deferred_families_return_501() {
    let addr = spawn(GatewayConfig::default()).await;

    let deferred = [
        // `operations` is a live family now (`REQ_0969`): SOVD async executions
        // are mounted per kind through the `ActionSink` seam, not deferred.
        // `configurations` is a live family now (`REQ_0971`): per-entity config
        // storage is mounted per kind through the same seam, not deferred.
        // `bulk-data` is a live family now (`REQ_0972`): apps/components expose a
        // real GET/POST/DELETE file surface through the `ActionSink` seam, so
        // `…/{id}/bulk-data` is no longer a wholesale-deferred path.
        // `locks` is a live family now (#149): `…/{id}/locks` is a real
        // POST/PUT/DELETE surface, so it is no longer a wholesale-deferred path.
        // `scripts` is a live family now (`REQ_0973`): apps/components expose a
        // real storage + executions surface through the `ActionSink` seam, so
        // `…/{id}/scripts` is no longer a wholesale-deferred path.
        // `status` (lifecycle-status) is a live family now (`REQ_0975`):
        // apps/components expose a real GET/PUT transition surface through the
        // `ActionSink` seam, so `…/{id}/status` is no longer a deferred path.
        // `logs` is a live family now (`REQ_0976`): all four kinds expose a real
        // `…/{id}/logs` read surface plus a `…/logs/configuration` GET/PUT, so it
        // is no longer a deferred path.
        // `triggers` is a live family now (`REQ_0962`): entity-scoped triggers are
        // mounted per kind, so `…/{id}/triggers` is a real surface, not deferred.
        // `cyclic-subscriptions` is a live family now (`REQ_0977`): apps,
        // components, and functions expose a real CRUD + per-resource SSE sample
        // surface, so `…/{id}/cyclic-subscriptions` is no longer a deferred path.
        // `updates` is a live family now (`REQ_0974`): the global software-update
        // surface is mounted at `/api/v1/updates`, so it is no longer deferred.
        // `/auth/token` is a real POST route now (#86); a bare `/auth` path with
        // no handler still declines via the deferred-family fallback.
        "/api/v1/auth",
        "/api/v1/docs",
        "/api/v1/functions/root/x-medkit-graph",
    ];

    for path in deferred {
        let (status, value) = get_json(addr, path).await;
        assert_eq!(status, 501, "{path} should be 501 (deferred), not 404");
        assert_eq!(
            value["error_code"], "not-implemented",
            "{path} must carry a contract-shaped 501 body"
        );
        assert!(value["parameters"]["family"].is_string());
    }
}

/// `TEST_0908` — CORS is advertised and the token-bucket rate limit throttles.
#[tokio::test]
async fn transport_hardening() {
    // CORS: a permissive default advertises an allow-origin header.
    let addr = spawn(GatewayConfig::default()).await;
    let resp = request_with_origin(addr, "GET", "/api/v1/", Some("http://example.com")).await;
    assert_eq!(resp.status, 200);
    assert!(
        resp.headers.contains("access-control-allow-origin"),
        "CORS allow-origin header should be present: {}",
        resp.headers
    );

    // Rate limit: capacity 1, no refill — second request is throttled.
    let config = GatewayConfig {
        cors: CorsConfig {
            enabled: false,
            allow_any_origin: false,
        },
        rate_limit: Some(RateLimit {
            capacity: 1,
            refill_per_second: 0,
        }),
        ..GatewayConfig::default()
    };
    let addr = spawn(config).await;
    let first = get(addr, "/api/v1/").await;
    assert_eq!(first.status, 200);
    let second = get(addr, "/api/v1/").await;
    assert_eq!(second.status, 429, "second request should be rate-limited");
}

/// Map the compile-time capture into the wire DTO — the wiring a real
/// deployment binary performs to inject build identity (`ADR_0132`).
fn captured_build_info() -> BuildInfo {
    let c = taktora_build_info::CAPTURED;
    BuildInfo {
        git_sha: c.git_sha.to_owned(),
        git_short: c.git_short.to_owned(),
        git_describe: c.git_describe.to_owned(),
        git_dirty: c.git_dirty,
        build_timestamp: c.build_timestamp.to_owned(),
        rustc_version: c.rustc_version.to_owned(),
    }
}

/// `TEST_0956` — build identity captured at compile time and injected through
/// the `with_build_info` seam surfaces under `vendor_info` at `/version-info`,
/// typed and additive.
#[tokio::test]
async fn version_info_reports_injected_build_identity() {
    let build = captured_build_info();
    let config = GatewayConfig::default().with_build_info(build.clone());
    let addr = spawn(config).await;

    let (status, value) = get_json(addr, "/api/v1/version-info").await;
    assert_eq!(status, 200);
    let vendor = &value["items"][0]["vendor_info"];

    // Existing fields intact — the additive change keeps a drop-in `ros2_medkit`
    // client working (`REQ_0911`); `TEST_0906`'s golden shape-match also passes.
    assert_eq!(vendor["name"], "taktora-medkit");
    // The injected identity is served verbatim.
    assert_eq!(vendor["git_sha"], build.git_sha);
    assert_eq!(vendor["git_short"], build.git_short);
    assert_eq!(vendor["git_describe"], build.git_describe);
    assert_eq!(vendor["build_timestamp"], build.build_timestamp);
    assert_eq!(vendor["rustc_version"], build.rustc_version);
    assert_eq!(vendor["git_dirty"], Value::Bool(build.git_dirty));
    // Types on the wire: git/timestamp/rustc are JSON strings, dirty is a bool.
    assert!(vendor["git_sha"].is_string());
    assert!(vendor["git_dirty"].is_boolean());
    // The capture actually ran against this repo — a real hash, not empty.
    assert!(!build.git_sha.is_empty());
}

/// `TEST_0956` — with no injected identity the document stays well-formed: git
/// fields report `"unknown"` and the tree reads clean (the no-`.git` fallback
/// shape, `REQ_0990`).
#[tokio::test]
async fn version_info_defaults_to_unknown_without_injection() {
    let addr = spawn(GatewayConfig::default()).await;
    let (status, value) = get_json(addr, "/api/v1/version-info").await;
    assert_eq!(status, 200);
    let vendor = &value["items"][0]["vendor_info"];
    assert_eq!(vendor["git_sha"], "unknown");
    assert_eq!(vendor["git_describe"], "unknown");
    assert_eq!(vendor["build_timestamp"], "unknown");
    assert_eq!(vendor["git_dirty"], Value::Bool(false));
}
