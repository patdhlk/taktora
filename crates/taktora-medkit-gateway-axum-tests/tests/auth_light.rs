//! Auth-light integration tests: drive a **live** axum server and exercise the
//! `/api/v1/auth/*` token endpoints and the enforcement=none resource posture
//! against real TCP (issue #86).
//!
//! - `TEST_0922` — `POST /api/v1/auth/token` with `client_credentials` returns a
//!   contract-shaped, JWT-shaped token response.
//! - `TEST_0923` — resource endpoints accept requests with **or** without a
//!   `Bearer` token (enforcement none) and never reject on auth.
//! - `TEST_0924` — the full client shape (login → token → read-core call with the
//!   token → 200) and the `Authenticator` seam: a strict, externally-defined
//!   impl is substitutable without touching handlers.
//!
//! Real JWT signing/validation, RBAC, and enforcement modes are deferred to
//! tracking issue #87 behind the `Authenticator` seam.

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{
    AuthCredentials, AuthRejection, AuthTokenResponse, Authenticator, GatewayConfig, demo,
    ephemeral_listener, router_with_authenticator, serve_listener,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Spawn a live server with the default (permissive) authenticator.
async fn spawn() -> SocketAddr {
    let gateway = Gateway::new(demo::provider());
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind ephemeral port");
    tokio::spawn(async move {
        let _ = serve_listener(listener, view, &GatewayConfig::default()).await;
    });
    addr
}

/// Spawn a live server with a caller-supplied authenticator, proving the seam is
/// substitutable from outside the crate without touching any handler.
async fn spawn_with(auth: Arc<dyn Authenticator>) -> SocketAddr {
    let gateway = Gateway::new(demo::provider());
    let view = Arc::new(gateway.view());
    let (listener, addr) = ephemeral_listener().await.expect("bind ephemeral port");
    let app = router_with_authenticator(view, &GatewayConfig::default(), auth);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

struct Response {
    status: u16,
    body: String,
}

/// Minimal HTTP/1.1 client supporting a request body and an optional bearer
/// header; one request per connection (`Connection: close`).
async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> Response {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let auth_header = bearer.map_or_else(String::new, |t| format!("Authorization: Bearer {t}\r\n"));
    let body_part = body.map_or_else(
        || "\r\n".to_owned(),
        |b| {
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
                b.len()
            )
        },
    );
    let req =
        format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth_header}Connection: close\r\n{body_part}");
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

const CLIENT_CREDENTIALS: &str =
    r#"{"grant_type":"client_credentials","client_id":"diag","client_secret":"s3cret"}"#;

/// `TEST_0922` — the token endpoint issues a contract-shaped, JWT-shaped token.
#[tokio::test]
async fn token_endpoint_issues_contract_shaped_jwt() {
    let addr = spawn().await;
    let resp = request(
        addr,
        "POST",
        "/api/v1/auth/token",
        None,
        Some(CLIENT_CREDENTIALS),
    )
    .await;
    assert_eq!(resp.status, 200, "token endpoint should be 200: {}", resp.body);
    let value = json(&resp.body);

    // Contract `AuthTokenResponse` required fields.
    let access_token = value["access_token"].as_str().expect("access_token string");
    assert_eq!(value["token_type"], "Bearer");
    assert!(value["expires_in"].is_number(), "expires_in must be an integer");
    assert!(value["scope"].is_string(), "scope must be a string");

    // JWT *shape* (not cryptographically real, deferred to #87): three
    // non-empty base64url segments.
    let segments: Vec<&str> = access_token.split('.').collect();
    assert_eq!(segments.len(), 3, "JWT must have 3 dot-separated segments");
    assert!(
        segments.iter().all(|s| !s.is_empty()),
        "every JWT segment must be non-empty: {access_token}"
    );
}

/// `TEST_0922` — `/auth/authorize` and `/auth/revoke` exist and are POST-only.
#[tokio::test]
async fn authorize_and_revoke_endpoints_exist() {
    let addr = spawn().await;

    let authorize = request(
        addr,
        "POST",
        "/api/v1/auth/authorize",
        None,
        Some(CLIENT_CREDENTIALS),
    )
    .await;
    assert_eq!(authorize.status, 200, "authorize: {}", authorize.body);
    assert_eq!(json(&authorize.body)["token_type"], "Bearer");

    let revoke = request(
        addr,
        "POST",
        "/api/v1/auth/revoke",
        None,
        Some(r#"{"token":"whatever"}"#),
    )
    .await;
    assert_eq!(revoke.status, 200, "revoke: {}", revoke.body);
    assert!(json(&revoke.body)["status"].is_string());

    // GET on a token route is not a contract method: 405, never a 200 or a body.
    let get = request(addr, "GET", "/api/v1/auth/token", None, None).await;
    assert_eq!(get.status, 405, "GET on /auth/token must be 405 Method Not Allowed");
}

/// `TEST_0923` — resource endpoints pass with or without a Bearer token
/// (enforcement none); auth never rejects a read in v1.
#[tokio::test]
async fn resource_endpoints_pass_with_and_without_bearer() {
    let addr = spawn().await;

    let no_token = request(addr, "GET", "/api/v1/", None, None).await;
    assert_eq!(no_token.status, 200, "read-core must pass without a token");

    let bogus_token = request(addr, "GET", "/api/v1/", Some("not-a-real-jwt"), None).await;
    assert_eq!(
        bogus_token.status, 200,
        "read-core must pass with an unverified Bearer token (enforcement none)"
    );
}

/// `TEST_0924` — the full client shape: login → obtain token → call a read-core
/// endpoint with the token → 200.
#[tokio::test]
async fn full_client_login_then_read_with_token() {
    let addr = spawn().await;

    let login = request(
        addr,
        "POST",
        "/api/v1/auth/token",
        None,
        Some(CLIENT_CREDENTIALS),
    )
    .await;
    assert_eq!(login.status, 200);
    let token = json(&login.body)["access_token"]
        .as_str()
        .expect("access_token")
        .to_owned();

    let read = request(addr, "GET", "/api/v1/components", Some(&token), None).await;
    assert_eq!(read.status, 200, "read-core call with the issued token must be 200");
    assert!(json(&read.body).is_object() || json(&read.body).is_array());
}

/// A strict, externally-defined `Authenticator` that rejects everything —
/// proves the seam is a real public trait, substitutable without touching any
/// handler. (Real strict auth lands in #87.)
struct RejectingAuthenticator;

impl Authenticator for RejectingAuthenticator {
    fn issue_token(&self, _credentials: &AuthCredentials) -> Result<AuthTokenResponse, AuthRejection> {
        Err(AuthRejection::invalid_client("rejected by test authenticator"))
    }

    fn verify_bearer(&self, _bearer: Option<&str>) -> Result<(), AuthRejection> {
        Err(AuthRejection::invalid_client("rejected by test authenticator"))
    }
}

/// `TEST_0924` — substituting a strict `Authenticator` changes token issuance
/// without touching handlers, and enforcement=none read-core is unaffected.
#[tokio::test]
async fn strict_authenticator_is_substitutable() {
    let addr = spawn_with(Arc::new(RejectingAuthenticator)).await;

    // The strict impl rejects token issuance.
    let login = request(
        addr,
        "POST",
        "/api/v1/auth/token",
        None,
        Some(CLIENT_CREDENTIALS),
    )
    .await;
    assert_eq!(login.status, 401, "strict authenticator must reject: {}", login.body);

    // Read-core is enforcement=none: still served, handlers unchanged.
    let read = request(addr, "GET", "/api/v1/", None, None).await;
    assert_eq!(read.status, 200, "enforcement=none read must still pass");
}
