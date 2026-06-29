//! Integration tests for the logs family (`REQ_0976`).
//!
//! Drives a live axum server whose read-model carries a few diagnostic log
//! entries (via [`MockProvider::with_log`]) and whose write seam is the default
//! in-memory [`SimActionSink`] (which stores log configuration). Exercises the
//! filtered `…/logs` read surface and the `…/logs/configuration` GET/PUT
//! round-trip over real TCP.
//!
//! - `TEST_0951` — GET `…/logs` lists every entry; `?severity=error` and
//!   `?context=<substr>` filter it; GET `…/logs/configuration` returns a default
//!   and a PUT round-trips through a subsequent GET.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, ephemeral_listener, serve_listener};
use taktora_medkit_provider::{LogEntry, MockProvider};
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

fn log(severity: &str, context: &str, message: &str) -> LogEntry {
    LogEntry {
        timestamp: 1_782_661_500.0,
        severity: severity.to_owned(),
        context: context.to_owned(),
        message: message.to_owned(),
    }
}

/// Spawn a live server whose read-model carries three log entries on `spark`.
async fn spawn() -> SocketAddr {
    let provider = MockProvider::new()
        .with_log("spark", log("info", "boot", "started"))
        .with_log("spark", log("error", "motor", "overheated"))
        .with_log("spark", log("warning", "motor", "running warm"));
    let view = Arc::new(Gateway::new(provider).view());
    let (listener, addr) = ephemeral_listener().await.expect("bind");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// `TEST_0951` — the log read surface filters by severity/context, and the log
/// configuration round-trips through the write seam.
#[tokio::test]
async fn logs_read_and_configuration_over_http() {
    let addr = spawn().await;
    let base = "/api/v1/components/spark/logs";

    // GET …/logs lists every entry.
    let all = get(addr, base).await;
    assert_eq!(all.status, 200, "all: {}", all.body);
    let all = json(&all.body);
    assert_eq!(all["x-medkit"]["total_count"], 3);

    // ?severity=error selects only the error entry.
    let errors = json(&get(addr, &format!("{base}?severity=error")).await.body);
    assert_eq!(errors["x-medkit"]["total_count"], 1);
    assert_eq!(errors["items"][0]["severity"], "error");
    assert_eq!(errors["items"][0]["message"], "overheated");

    // ?context=motor selects only the two motor entries (substring match).
    let motor = json(&get(addr, &format!("{base}?context=motor")).await.body);
    assert_eq!(motor["x-medkit"]["total_count"], 2);

    // GET …/logs/configuration returns the default configuration.
    let config_path = format!("{base}/configuration");
    let default = get(addr, &config_path).await;
    assert_eq!(default.status, 200);
    assert_eq!(json(&default.body)["default_level"], "info");

    // PUT a new configuration -> 200; a subsequent GET reflects it.
    let put = request(
        addr,
        "PUT",
        &config_path,
        Some(r#"{"default_level":"debug"}"#),
    )
    .await;
    assert_eq!(put.status, 200, "put: {}", put.body);
    assert_eq!(json(&put.body)["default_level"], "debug");
    assert_eq!(
        json(&get(addr, &config_path).await.body)["default_level"],
        "debug"
    );
}
