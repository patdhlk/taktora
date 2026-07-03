//! Server configuration: the bind address and the transport-hardening knobs
//! (CORS, rate limit, optional TLS), all with documented defaults and all off
//! the control path (`REQ_0919`).

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use taktora_medkit_manifest::Manifest;
use taktora_medkit_model::BuildInfo;

/// The documented default bind: loopback, port 8080.
///
/// `ros2_medkit` binds `0.0.0.0:8080`; the dev default here is loopback so a
/// freshly-started skeleton is not reachable off-host by accident. Override
/// [`GatewayConfig::bind`] to expose it.
pub const DEFAULT_BIND: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

/// How the gateway answers cross-origin requests.
///
/// Default: enabled and permissive (any origin, the read-only `GET`/`DELETE`
/// methods the surface uses) so a browser-based diagnostic client works out of
/// the box. Set `enabled` to `false` to mount no CORS layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorsConfig {
    /// Whether to mount a CORS layer at all.
    pub enabled: bool,
    /// Whether to allow any origin (`*`). When `false`, no origin is allowed.
    pub allow_any_origin: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_any_origin: true,
        }
    }
}

/// A token-bucket rate limit applied across all clients.
///
/// `capacity` is the burst size; `refill_per_second` is the steady-state rate.
/// Rate limiting is **off by default** ([`GatewayConfig::rate_limit`] is `None`)
/// so it never throttles the control-plane-free read path unless asked for.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RateLimit {
    /// Maximum tokens (burst size); must be positive to have any effect.
    pub capacity: u32,
    /// Tokens replenished per second.
    pub refill_per_second: u32,
}

/// Paths to the PEM-encoded certificate chain and private key for TLS.
///
/// Serving over TLS additionally requires the crate's `tls` feature; with the
/// feature off, [`serve`](crate::serve) returns an error rather than silently
/// downgrading to plaintext. Default: no TLS (plaintext).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TlsConfig {
    /// Path to the PEM certificate chain.
    pub cert_path: PathBuf,
    /// Path to the PEM private key.
    pub key_path: PathBuf,
}

/// The full server configuration.
///
/// [`GatewayConfig::default`] yields the documented dev defaults: bind
/// `127.0.0.1:8080`, permissive CORS, no rate limit, no TLS, and no manifest
/// (flat grouping).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayConfig {
    /// The socket address to bind.
    pub bind: SocketAddr,
    /// Cross-origin policy.
    pub cors: CorsConfig,
    /// Optional token-bucket rate limit (off when `None`).
    pub rate_limit: Option<RateLimit>,
    /// Optional TLS (plaintext when `None`).
    pub tls: Option<TlsConfig>,
    /// Whether the `/api/v1/auth/*` token endpoints are mounted (`REQ_0968`).
    ///
    /// Default `true`: the dev-friendly permissive [`Authenticator`](crate::Authenticator) is mounted
    /// and answers `200`. Set `false` for **demo parity** with an upstream
    /// `ros2_medkit` started with auth disabled — the auth routes then answer a
    /// contract-shaped `404` (the family is *absent*, not deferred), so a client
    /// probing `/auth/*` learns auth is off rather than receiving an unusable
    /// token. Enforcement of issued tokens (real JWT/RBAC) stays deferred to #87
    /// regardless of this flag.
    pub auth_enabled: bool,
    /// The grouping [`Manifest`] applied when building the read-model: declared
    /// Areas/Components become entities and the binding-emitted raw entities are
    /// re-parented under them (`REQ_0921`). `None` or empty leaves grouping flat
    /// (`REQ_0922`). Load one with [`Manifest::from_toml`] so ops can edit the
    /// topology in a `medkit.toml` without recompiling.
    pub manifest: Option<Manifest>,
    /// Source identity of the running binary, reported under `vendor_info` in
    /// `GET /api/v1/version-info` (`REQ_0990`).
    ///
    /// The default is all-`"unknown"`. A deployment binary captures the real
    /// identity with the `taktora-build-info` crate and injects it here (see
    /// [`GatewayConfig::with_build_info`]); the extractable core never depends on
    /// that crate — build identity arrives as data (`ADR_0132`).
    pub build_info: BuildInfo,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND,
            cors: CorsConfig::default(),
            rate_limit: None,
            tls: None,
            auth_enabled: true,
            manifest: None,
            build_info: BuildInfo::default(),
        }
    }
}

impl GatewayConfig {
    /// Set the build identity reported at `/version-info` (`REQ_0990`).
    ///
    /// A deployment binary calls this with a [`BuildInfo`] mapped from
    /// `taktora_build_info::CAPTURED`, so the served document names the exact
    /// commit the binary was built from.
    #[must_use]
    pub fn with_build_info(mut self, build_info: BuildInfo) -> Self {
        self.build_info = build_info;
        self
    }
}
