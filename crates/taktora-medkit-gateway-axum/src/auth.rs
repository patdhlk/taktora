//! Auth-light seam for the gateway (`REQ_0935`–`REQ_0939`, `ADR_0118`,
//! `BB_0112`).
//!
//! A drop-in SOVD client authenticates via `OAuth2` `client_credentials` → JWT
//! `Bearer` **before** it reads any diagnostics. v1 ships that login flow behind
//! a seam without the heavyweight machinery: the [`Authenticator`] trait is the
//! substitution point, the default [`PermissiveAuthenticator`] is dev-mode (any
//! credentials succeed, any/no token accepted), and resource routes run
//! **enforcement = none** — a `Bearer` token is accepted and never verified, and
//! requests with or without one always pass auth.
//!
//! The issued token is **shape-valid, not cryptographically real**: a
//! hand-rolled `base64url(header).base64url(payload).signature` string with an
//! `alg: "none"` header and a placeholder signature segment. Real JWT signing +
//! validation, RBAC (`viewer`/`operator`/`configurator`/`admin`), and the
//! enforcement modes (`none`/`write`/`all`) are deferred to tracking issue #87,
//! behind this seam, so a strict impl can drop in later without reworking any
//! handler.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::{Deserialize, Serialize};
use taktora_medkit_model::GenericError;

/// `OAuth2`-style token request body (contract `AuthCredentials`).
///
/// `grant_type` is the only required field; `client_credentials` is the v1
/// flow. The other fields are accepted permissively and (in dev mode) echoed or
/// ignored.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AuthCredentials {
    /// The `OAuth2` grant type, e.g. `client_credentials` or `refresh_token`.
    pub grant_type: String,
    /// The client identifier, if presented.
    #[serde(default)]
    pub client_id: Option<String>,
    /// The client secret, if presented (never logged).
    #[serde(default)]
    pub client_secret: Option<String>,
    /// A refresh token, for the `refresh_token` grant.
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// The requested scope, space-delimited.
    #[serde(default)]
    pub scope: Option<String>,
}

/// The token response (contract `AuthTokenResponse`).
///
/// `access_token`, `token_type`, `expires_in`, and `scope` are always present;
/// `refresh_token` is optional and omitted in v1.
#[derive(Clone, Debug, Serialize)]
pub struct AuthTokenResponse {
    /// The issued (shape-valid, unsigned) JWT.
    pub access_token: String,
    /// Always `"Bearer"`.
    pub token_type: String,
    /// Token lifetime in seconds.
    pub expires_in: u64,
    /// The granted scope, space-delimited.
    pub scope: String,
    /// A refresh token, when issued (`None` in v1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// A token-revocation request (contract `AuthRevokeRequest`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AuthRevokeRequest {
    /// The token to revoke.
    #[serde(default)]
    pub token: String,
    /// An optional hint at the token's type (`access_token` / `refresh_token`).
    #[serde(default)]
    pub token_type_hint: Option<String>,
}

/// A token-revocation response (contract `AuthRevokeResponse`).
#[derive(Clone, Debug, Serialize)]
pub struct AuthRevokeResponse {
    /// The revocation status, e.g. `"revoked"`.
    pub status: String,
}

/// A rejection from a strict [`Authenticator`]. The permissive default never
/// returns one; it exists so a #87 strict impl renders a contract-shaped
/// `GenericError` without reworking handlers.
#[derive(Clone, Debug)]
pub struct AuthRejection {
    status: StatusCode,
    error_code: String,
    message: String,
}

impl AuthRejection {
    /// A `401 Unauthorized` for invalid or rejected client credentials.
    #[must_use]
    pub fn invalid_client(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error_code: "invalid-client".to_owned(),
            message: message.into(),
        }
    }

    /// A `400 Bad Request` for an unsupported or missing grant type.
    #[must_use]
    pub fn unsupported_grant_type(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error_code: "unsupported-grant-type".to_owned(),
            message: message.into(),
        }
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        let body = GenericError {
            error_code: self.error_code,
            message: self.message,
            parameters: BTreeMap::new(),
        };
        (self.status, Json(body)).into_response()
    }
}

/// The auth seam: token issuance and bearer verification, behind one trait so a
/// strict JWT/RBAC impl (#87) substitutes for the permissive default without
/// touching any handler (`REQ_0937`, `BB_0112`).
pub trait Authenticator: Send + Sync {
    /// Issue an access token for `credentials`.
    ///
    /// # Errors
    ///
    /// Returns an [`AuthRejection`] when the impl declines the credentials. The
    /// permissive default never declines.
    fn issue_token(
        &self,
        credentials: &AuthCredentials,
    ) -> Result<AuthTokenResponse, AuthRejection>;

    /// Verify a presented `Bearer` token (without the `Bearer ` prefix), if any.
    ///
    /// Under enforcement = none the result is advisory — resource routes do not
    /// consult it — but the seam carries it so a strict impl (#87) can enforce.
    ///
    /// # Errors
    ///
    /// Returns an [`AuthRejection`] when the impl rejects the token.
    fn verify_bearer(&self, bearer: Option<&str>) -> Result<(), AuthRejection>;
}

/// The default dev-mode authenticator: any credentials succeed and any (or no)
/// `Bearer` token is accepted (`REQ_0936`). Issues a shape-valid, unsigned JWT.
#[derive(Clone, Debug, Default)]
pub struct PermissiveAuthenticator;

/// The token lifetime advertised by the permissive issuer (one hour).
const DEFAULT_EXPIRES_IN: u64 = 3600;
/// The scope granted when the client requests none.
const DEFAULT_SCOPE: &str = "medkit:read";

impl Authenticator for PermissiveAuthenticator {
    fn issue_token(
        &self,
        credentials: &AuthCredentials,
    ) -> Result<AuthTokenResponse, AuthRejection> {
        let scope = credentials
            .scope
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_SCOPE.to_owned());
        let subject = credentials.client_id.as_deref().unwrap_or("dev-client");
        Ok(AuthTokenResponse {
            access_token: dev_jwt(subject, &scope, DEFAULT_EXPIRES_IN),
            token_type: "Bearer".to_owned(),
            expires_in: DEFAULT_EXPIRES_IN,
            scope,
            refresh_token: None,
        })
    }

    fn verify_bearer(&self, _bearer: Option<&str>) -> Result<(), AuthRejection> {
        Ok(())
    }
}

/// Mint a **shape-valid, unsigned** JWT: `base64url(header).base64url(payload)`
/// plus a fixed placeholder signature segment so the string is three non-empty
/// dot-separated segments. The header declares `alg: "none"`; the payload is a
/// minimal claims set. Not cryptographically real — real signing is #87.
fn dev_jwt(subject: &str, scope: &str, expires_in: u64) -> String {
    let header = base64url(br#"{"alg":"none","typ":"JWT"}"#);
    let claims = format!(
        r#"{{"sub":{sub},"scope":{scope},"token_use":"access","exp_in":{expires_in}}}"#,
        sub = json_string(subject),
        scope = json_string(scope),
    );
    let payload = base64url(claims.as_bytes());
    // `alg: none` tokens carry an empty signature; we emit a constant non-empty
    // placeholder so the string keeps the three-segment JWT shape clients parse.
    let signature = base64url(b"unsigned-dev-token");
    format!("{header}.{payload}.{signature}")
}

/// Serialize a string as a JSON string literal (escaping `"` and `\`), so the
/// hand-built claims JSON stays valid for arbitrary client IDs/scopes.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Base64url (RFC 4648 §5) without padding — enough to make a JWT-shaped token
/// without a base64 dependency.
fn base64url(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        let n = chunk.len();
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if n > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        }
        if n > 2 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        }
    }
    out
}

/// Shared state for the `/auth/*` routes: the active authenticator behind the
/// seam.
type AuthState = Arc<dyn Authenticator>;

async fn token_handler(
    State(auth): State<AuthState>,
    credentials: Json<AuthCredentials>,
) -> Response {
    match auth.issue_token(&credentials) {
        Ok(token) => (StatusCode::OK, Json(token)).into_response(),
        Err(rejection) => rejection.into_response(),
    }
}

async fn revoke_handler() -> Response {
    // Permissive: revocation always succeeds (there is no real token store yet).
    // The request body is accepted but not required to parse in v1.
    (
        StatusCode::OK,
        Json(AuthRevokeResponse {
            status: "revoked".to_owned(),
        }),
    )
        .into_response()
}

/// The `/api/v1/auth/*` sub-router, parameterised on the active authenticator.
///
/// Three POST-only endpoints pinned from the contract (`contract/NOTES.md`):
/// `authorize`, `token` (singular), `revoke` (`REQ_0935`). Mounting them carves
/// the real routes out from under the `501` fallback for the `auth` family.
pub fn auth_router(api_base: &str, auth: AuthState) -> Router {
    Router::new()
        .route(&format!("{api_base}/auth/token"), post(token_handler))
        .route(&format!("{api_base}/auth/authorize"), post(token_handler))
        .route(&format!("{api_base}/auth/revoke"), post(revoke_handler))
        .with_state(auth)
}

/// A contract-shaped `404` for an `/auth/*` path (`REQ_0968`). Used when auth is
/// disabled, so the family reads as *absent* rather than deferred (`501`).
async fn auth_absent() -> Response {
    let body = GenericError {
        error_code: "not-found".to_owned(),
        message: "Authentication is not enabled on this gateway".to_owned(),
        parameters: BTreeMap::new(),
    };
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

/// The `/api/v1/auth/*` routes when auth is **disabled** (`REQ_0968`): the three
/// paths are bound to a `404` so the surface matches an upstream `ros2_medkit`
/// started with auth off, instead of falling through to the `501` deferred
/// fallback (which would mis-signal "not yet implemented").
pub fn auth_disabled_router(api_base: &str) -> Router {
    Router::new()
        .route(&format!("{api_base}/auth/token"), post(auth_absent))
        .route(&format!("{api_base}/auth/authorize"), post(auth_absent))
        .route(&format!("{api_base}/auth/revoke"), post(auth_absent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> AuthCredentials {
        AuthCredentials {
            grant_type: "client_credentials".to_owned(),
            client_id: Some("diag".to_owned()),
            client_secret: Some("s3cret".to_owned()),
            ..AuthCredentials::default()
        }
    }

    #[test]
    fn permissive_issues_a_jwt_shaped_bearer_token() {
        let token = PermissiveAuthenticator
            .issue_token(&creds())
            .expect("permissive never rejects");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.expires_in, DEFAULT_EXPIRES_IN);
        assert!(!token.scope.is_empty());
        let segments: Vec<&str> = token.access_token.split('.').collect();
        assert_eq!(segments.len(), 3, "JWT shape: 3 segments");
        assert!(segments.iter().all(|s| !s.is_empty()));
    }

    #[test]
    fn permissive_accepts_any_or_no_bearer() {
        assert!(PermissiveAuthenticator.verify_bearer(None).is_ok());
        assert!(
            PermissiveAuthenticator
                .verify_bearer(Some("anything"))
                .is_ok()
        );
    }

    #[test]
    fn requested_scope_is_echoed() {
        let mut c = creds();
        c.scope = Some("medkit:read medkit:write".to_owned());
        let token = PermissiveAuthenticator.issue_token(&c).unwrap();
        assert_eq!(token.scope, "medkit:read medkit:write");
    }

    /// The seam is object-safe and substitutable: a rejecting impl drops in
    /// behind `dyn Authenticator` without any handler change (`REQ_0937`).
    #[test]
    fn rejecting_authenticator_is_substitutable() {
        struct Rejecting;
        impl Authenticator for Rejecting {
            fn issue_token(
                &self,
                _c: &AuthCredentials,
            ) -> Result<AuthTokenResponse, AuthRejection> {
                Err(AuthRejection::invalid_client("nope"))
            }
            fn verify_bearer(&self, _b: Option<&str>) -> Result<(), AuthRejection> {
                Err(AuthRejection::invalid_client("nope"))
            }
        }
        let strict: Arc<dyn Authenticator> = Arc::new(Rejecting);
        let permissive: Arc<dyn Authenticator> = Arc::new(PermissiveAuthenticator);
        assert!(strict.issue_token(&creds()).is_err());
        assert!(permissive.issue_token(&creds()).is_ok());
    }

    #[test]
    fn base64url_matches_known_vectors() {
        // RFC 4648 test vectors (unpadded).
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
    }
}
