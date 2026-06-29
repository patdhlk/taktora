//! axum HTTP surface for `taktora-medkit`: the walking skeleton.
//!
//! Mounts the pure read-family resolvers of `taktora-medkit-gateway` over HTTP
//! on the `/api/v1` prefix, serving the SOVD read-diagnostic core
//! (entity tree, relationships, faults, data) backed by any
//! [`Provider`] — the in-process `MockProvider` for the skeleton
//! (`REQ_0917`). Deferred families answer a
//! contract-shaped `501 Not Implemented` so a path-hardcoding client gets a
//! clean, documented decline rather than a `404` or a parse error
//! (`REQ_0918`). Baseline transport hardening — CORS, a token-bucket rate
//! limit, and optional TLS — is folded in here, all configurable and all off
//! the control path (`REQ_0919`).
//!
//! axum and tokio are not taktora dependencies, so this crate stays part of the
//! extractable core (`REQ_0916`, `ADR_0111`).
//!
//! # Wiring for downstream slices
//!
//! The server holds an `Arc<MergedView>` built once from the provider's
//! snapshot via the [`MergePipeline`](taktora_medkit_gateway::MergePipeline).
//! A later slice that needs live refresh swaps that `Arc` for a hot-swappable
//! handle; #82 applies a manifest inside the pipeline; #83/#84 contribute extra
//! `ProviderSnapshot`s to merge. None of that changes the HTTP surface.

mod actions;
mod auth;
mod bulkdata;
mod config;
mod configurations;
mod cyclic;
pub mod demo;
mod error;
mod lifecycle;
mod locks;
mod logs;
mod ratelimit;
mod scripts;
mod triggers;
mod updates;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode, Uri};
use axum::routing::get;
use serde::Serialize;
use serde_json::{Value, json};
use taktora_medkit_gateway::view::{API_BASE, root_document, version_info_document};
use taktora_medkit_gateway::{FaultStatusFilter, Gateway, MergedView, collection_segment};
use taktora_medkit_model::EntityKind;
use taktora_medkit_provider::{ActionSink, Provider, Relation};
use tower_http::cors::{Any, CorsLayer};

use triggers::ServerState;

pub use auth::{
    AuthCredentials, AuthRejection, AuthRevokeRequest, AuthRevokeResponse, AuthTokenResponse,
    Authenticator, PermissiveAuthenticator,
};
pub use config::{CorsConfig, DEFAULT_BIND, GatewayConfig, RateLimit, TlsConfig};
pub use error::ApiError;
pub use ratelimit::TokenBucket;

type ApiResult = Result<Json<Value>, ApiError>;

/// The read-model handle the read-core handlers extract (via [`ServerState`]'s
/// [`FromRef`](axum::extract::FromRef) impl, so a hot-swap stays transparent).
type AppState = Arc<MergedView>;

fn ok<T: Serialize>(value: &T) -> Json<Value> {
    Json(serde_json::to_value(value).unwrap_or(Value::Null))
}

// ---- Read-core handlers (thin adapters over the pure resolvers) ------------

fn entity_detail(view: &MergedView, kind: EntityKind, id: &str) -> ApiResult {
    let entity = view
        .entity(id)
        .filter(|e| e.kind == kind)
        .ok_or_else(|| ApiError::NotFound(not_found_entity(id)))?;
    let collection = collection_segment(kind);
    let mut doc = serde_json::to_value(entity).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut doc {
        map.insert(
            "_links".to_owned(),
            json!({
                "self": format!("{API_BASE}/{collection}/{id}"),
                "collection": format!("{API_BASE}/{collection}")
            }),
        );
    }
    Ok(Json(doc))
}

fn not_found_entity(id: &str) -> taktora_medkit_model::GenericError {
    taktora_medkit_model::GenericError {
        error_code: "entity-not-found".to_owned(),
        message: "Entity not found".to_owned(),
        parameters: std::collections::BTreeMap::from([("entity_id".to_owned(), id.to_owned())]),
    }
}

fn status_filter(uri: &Uri) -> Result<FaultStatusFilter, ApiError> {
    let value = uri.query().and_then(|q| {
        q.split('&').find_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            (parts.next()? == "status").then(|| parts.next().unwrap_or(""))
        })
    });
    FaultStatusFilter::parse(value).ok_or_else(|| ApiError::bad_status(value.unwrap_or("")))
}

/// The fallback for any unmatched path: a deferred family declines with `501`,
/// never `404`, so a path-hardcoding client gets a clean documented decline.
async fn deferred(uri: Uri) -> ApiError {
    let path = uri.path().to_owned();
    ApiError::not_implemented(infer_family(&path), path)
}

fn infer_family(path: &str) -> String {
    const FAMILIES: &[&str] = &[
        "operations",
        "configurations",
        "bulk-data",
        "locks",
        "scripts",
        "updates",
        "triggers",
        "cyclic-subscriptions",
        "logs",
        "status",
        "auth",
        "docs",
    ];
    if let Some(found) = FAMILIES.iter().find(|f| path.contains(*f)) {
        return (*found).to_owned();
    }
    if path.contains("x-medkit") {
        return "x-medkit".to_owned();
    }
    "unknown".to_owned()
}

// ---- Router assembly -------------------------------------------------------

fn kind_routes(kind: EntityKind, relations: &'static [Relation]) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}", collection_segment(kind));
    let mut router = Router::new()
        .route(
            &base,
            get(move |State(view): State<AppState>| async move { ok(&view.list(kind)) }),
        )
        .route(
            &format!("{base}/{{id}}"),
            get(
                move |State(view): State<AppState>, Path(id): Path<String>| async move {
                    entity_detail(&view, kind, &id)
                },
            ),
        )
        .route(
            &format!("{base}/{{id}}/faults"),
            get(
                move |State(view): State<AppState>, Path(id): Path<String>, uri: Uri| async move {
                    let filter = status_filter(&uri)?;
                    view.entity_faults(kind, &id, filter)
                        .map(|list| ok(&list))
                        .map_err(ApiError::from)
                },
            ),
        )
        .route(
            &format!("{base}/{{id}}/faults/{{fault_code}}"),
            get(
                move |State(view): State<AppState>, Path((id, code)): Path<(String, String)>| async move {
                    view.fault_detail(kind, &id, &code)
                        .map(|detail| ok(&detail))
                        .map_err(ApiError::from)
                },
            )
            .delete(
                move |State(view): State<AppState>, Path((id, code)): Path<(String, String)>| async move {
                    view.delete_fault(kind, &id, &code)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(ApiError::from)
                },
            ),
        )
        .route(
            &format!("{base}/{{id}}/data"),
            get(
                move |State(view): State<AppState>, Path(id): Path<String>| async move {
                    view.data(kind, &id, None)
                        .map(Json)
                        .map_err(ApiError::from)
                },
            ),
        )
        .route(
            &format!("{base}/{{id}}/data/{{*topic}}"),
            get(
                move |State(view): State<AppState>, Path((id, topic)): Path<(String, String)>| async move {
                    view.data(kind, &id, Some(&topic))
                        .map(Json)
                        .map_err(ApiError::from)
                },
            ),
        );

    for &relation in relations {
        router = router.route(
            &format!("{base}/{{id}}/{}", relation.segment()),
            get(
                move |State(view): State<AppState>, Path(id): Path<String>| async move {
                    view.relationship(kind, &id, relation)
                        .map(|collection| ok(&collection))
                        .map_err(ApiError::from)
                },
            ),
        );
    }
    router
}

fn api_router() -> Router<ServerState> {
    let root = get(|| async { Json(root_document()) });
    Router::new()
        // The contract's canonical root is `/api/v1/` (trailing slash); accept
        // the bare prefix too.
        .route(API_BASE, root.clone())
        .route(&format!("{API_BASE}/"), root)
        .route(
            &format!("{API_BASE}/version-info"),
            get(|| async { Json(version_info_document()) }),
        )
        .route(
            &format!("{API_BASE}/health"),
            get(|State(view): State<AppState>| async move {
                // The structural blocks come from the (clock-free, testable)
                // resolver; the wall-clock `timestamp` is stamped here at the
                // edge so the golden's field is present (`REQ_0967`).
                let mut doc = view.health_document();
                if let Value::Object(map) = &mut doc {
                    let nanos = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos());
                    map.insert(
                        "timestamp".to_owned(),
                        json!(u64::try_from(nanos).unwrap_or(u64::MAX)),
                    );
                }
                Json(doc)
            }),
        )
        .route(
            &format!("{API_BASE}/faults"),
            get(|State(view): State<AppState>, uri: Uri| async move {
                let filter = status_filter(&uri)?;
                Ok::<_, ApiError>(ok(&view.global_faults(filter)))
            })
            // Global clear-all (`REQ_0964`). The read-only skeleton cannot mutate
            // the snapshot, so — like the per-entity fault DELETE — this is a
            // shape-compatible `204` acknowledgement, not a write-through. A real
            // clear lands with the binding write-path under the `ADR_0119` gate.
            .delete(|| async { StatusCode::NO_CONTENT }),
        )
        // The contract's canonical **global** fault SSE stream (`REQ_0961`); the
        // trigger-filtered stream stays at `/triggers/events`.
        .route(
            &format!("{API_BASE}/faults/stream"),
            get(triggers::faults_stream),
        )
        .merge(kind_routes(
            EntityKind::Area,
            &[Relation::Contains, Relation::Components],
        ))
        .merge(kind_routes(
            EntityKind::Component,
            &[
                Relation::Subcomponents,
                Relation::Hosts,
                Relation::DependsOn,
            ],
        ))
        .merge(kind_routes(
            EntityKind::App,
            &[
                Relation::IsLocatedOn,
                Relation::BelongsTo,
                Relation::DependsOn,
            ],
        ))
        .merge(kind_routes(EntityKind::Function, &[Relation::Hosts]))
        // Triggers + the SSE event stream (`REQ_0930`–`REQ_0934`), carved out
        // from under the `deferred` fallback that still `501`s the entity-scoped
        // `…/{id}/triggers` path. The static `/triggers/events` route binds ahead
        // of the `/triggers/{id}` capture.
        .route(
            &format!("{API_BASE}/triggers"),
            get(triggers::list_triggers).post(triggers::create_trigger),
        )
        .route(
            &format!("{API_BASE}/triggers/events"),
            get(triggers::events_stream),
        )
        .route(
            &format!("{API_BASE}/triggers/{{id}}"),
            get(triggers::get_trigger).delete(triggers::delete_trigger),
        )
        // Entity-scoped triggers (`REQ_0962`): the contract mounts triggers per
        // entity (`/{collection}/{id}/triggers…`), so a path-hardcoding client
        // reaches them where it expects. Backed by the same store as the global
        // surface above; carved out from under the `deferred` `501` fallback.
        .merge(triggers::trigger_routes(EntityKind::Area))
        .merge(triggers::trigger_routes(EntityKind::Component))
        .merge(triggers::trigger_routes(EntityKind::App))
        .merge(triggers::trigger_routes(EntityKind::Function))
        // Cyclic subscriptions (`REQ_0977`): periodic data-sampling subscriptions
        // with a per-resource SSE sample stream, mounted per entity on the three
        // kinds the contract exposes them on (apps, components, functions) and
        // carved out from under the `deferred` `501` fallback. Each subscription's
        // `…/events` stream samples the entity's data on its own cadence — a
        // self-contained periodic stream, distinct from the trigger broadcast.
        .merge(cyclic::cyclic_routes(EntityKind::Component))
        .merge(cyclic::cyclic_routes(EntityKind::App))
        .merge(cyclic::cyclic_routes(EntityKind::Function))
        // SOVD diagnostic-scoped exclusive access (`REQ_0940`–`REQ_0944`, issue
        // #149), carved out from under the `deferred` fallback for the two entity
        // kinds the contract exposes `/locks` on (apps, components). Guards no SC
        // resource — strictly QM coordination between diagnostic clients
        // (`ADR_0120`).
        .merge(locks::lock_routes(EntityKind::App))
        .merge(locks::lock_routes(EntityKind::Component))
        // Operations: SOVD async action executions (`REQ_0969`, `REQ_0970`),
        // served through the `ActionSink` seam and carved out from under the
        // `deferred` `501` fallback for the kinds the contract exposes operations
        // on. v1 is backed by an in-memory simulation that performs no real
        // effect; the write-surface safety gate (`ADR_0119`) re-enters at the
        // seam when a real binding lands (`ADR_0126`).
        .merge(actions::operation_routes(EntityKind::Area))
        .merge(actions::operation_routes(EntityKind::Component))
        .merge(actions::operation_routes(EntityKind::App))
        .merge(actions::operation_routes(EntityKind::Function))
        // Configurations: SOVD per-entity configuration storage (`REQ_0971`),
        // served through the same `ActionSink` seam and carved out from under the
        // `deferred` `501` fallback for the kinds the contract exposes
        // configurations on. v1 is an in-memory simulation (no real effect); the
        // write-surface safety gate (`ADR_0119`) re-enters at the seam when a real
        // binding lands (`ADR_0126`).
        .merge(configurations::configuration_routes(EntityKind::Area))
        .merge(configurations::configuration_routes(EntityKind::Component))
        .merge(configurations::configuration_routes(EntityKind::App))
        .merge(configurations::configuration_routes(EntityKind::Function))
        // Bulk-data: SOVD per-entity opaque file storage (`REQ_0972`), served
        // through the same `ActionSink` seam and carved out from under the
        // `deferred` `501` fallback for the two kinds the contract exposes
        // writable bulk-data on (apps, components). v1 is an in-memory simulation
        // (the body is stored opaquely as bytes, no real effect); the write-surface
        // safety gate (`ADR_0119`) re-enters at the seam when a real binding lands
        // (`ADR_0126`).
        .merge(bulkdata::bulk_data_routes(EntityKind::App))
        .merge(bulkdata::bulk_data_routes(EntityKind::Component))
        // Scripts: SOVD per-entity script storage plus async executions
        // (`REQ_0973`), a hybrid of the bulk-data upload surface and the
        // operations execution surface, served through the same `ActionSink` seam
        // and carved out from under the `deferred` `501` fallback for the two
        // kinds the contract exposes scripts on (apps, components). v1 is an
        // in-memory simulation (the body is stored opaquely as bytes, executions
        // complete synchronously, no real effect); the write-surface safety gate
        // (`ADR_0119`) re-enters at the seam when a real binding lands (`ADR_0126`).
        .merge(scripts::script_routes(EntityKind::App))
        .merge(scripts::script_routes(EntityKind::Component))
        // Updates: the SOVD software-update surface (`REQ_0974`), served through
        // the same `ActionSink` seam. Unlike the per-entity write families this is
        // a **global** family — the contract mounts it at the top level
        // (`/api/v1/updates…`), so it is mounted once, not per kind. v1 is an
        // in-memory simulation (lifecycle transitions in memory, no real effect);
        // the write-surface safety gate (`ADR_0119`) re-enters at the seam when a
        // real binding lands (`ADR_0126`).
        .merge(updates::update_routes())
        // Lifecycle-status: SOVD per-entity start/restart/shutdown transitions
        // (`REQ_0975`), served through the same `ActionSink` seam and carved out
        // from under the `deferred` `501` fallback for the two kinds the contract
        // exposes `/status` on (apps, components). v1 is an in-memory simulation
        // (transitions tracked in memory, no real effect); the write-surface
        // safety gate (`ADR_0119`) re-enters at the seam when a real binding lands
        // (`ADR_0126`).
        .merge(lifecycle::lifecycle_routes(EntityKind::App))
        .merge(lifecycle::lifecycle_routes(EntityKind::Component))
        // Logs: SOVD per-entity diagnostic logs (`REQ_0976`). The `…/logs` entry
        // list rides the **read** seam (the `MergedView` snapshot, like `…/data`);
        // the `…/logs/configuration` GET/PUT rides the `ActionSink` write seam.
        // Mounted on all four entity kinds and carved out from under the
        // `deferred` `501` fallback.
        .merge(logs::log_routes(EntityKind::Area))
        .merge(logs::log_routes(EntityKind::Component))
        .merge(logs::log_routes(EntityKind::App))
        .merge(logs::log_routes(EntityKind::Function))
        .fallback(deferred)
}

fn cors_layer(config: &CorsConfig) -> Option<CorsLayer> {
    if !config.enabled {
        return None;
    }
    let mut layer = CorsLayer::new().allow_methods([Method::GET, Method::POST, Method::DELETE]);
    if config.allow_any_origin {
        layer = layer.allow_origin(Any);
    }
    Some(layer)
}

/// Apply the configured transport-hardening layers over a state-bound router,
/// mounting the `/api/v1/auth/*` token endpoints behind `authenticator`.
///
/// The read-core handlers are identical regardless of which authenticator is
/// supplied — auth lives in the `/api/v1/auth/*` sub-router's own state — so a
/// strict JWT/RBAC impl (deferred to #87) substitutes for the permissive
/// default without touching any handler (`REQ_0937`, `BB_0112`).
fn router_with_state(
    state: ServerState,
    config: &GatewayConfig,
    authenticator: Arc<dyn Authenticator>,
) -> Router {
    let auth_routes = if config.auth_enabled {
        auth::auth_router(API_BASE, authenticator)
    } else {
        // Demo parity: auth off → `/auth/*` is absent (`404`), not deferred
        // (`501`) (`REQ_0968`).
        auth::auth_disabled_router(API_BASE)
    };
    let mut app = api_router().with_state(state).merge(auth_routes);

    if let Some(layer) = cors_layer(&config.cors) {
        app = app.layer(layer);
    }
    if let Some(limit) = config.rate_limit {
        let bucket = Arc::new(TokenBucket::new(limit));
        app = app.layer(axum::middleware::from_fn(move |request, next| {
            let bucket = Arc::clone(&bucket);
            async move { ratelimit::enforce(&bucket, request, next).await }
        }));
    }
    app
}

/// Build the axum application serving `view` under `/api/v1`, with the
/// configured transport-hardening layers applied.
///
/// This is the static surface: the trigger CRUD and SSE endpoints are mounted,
/// but no refresh-and-diff loop drives the stream (the served `view` is fixed).
/// Use [`serve_listener_with_provider`] for the live, event-emitting surface.
/// The `/api/v1/auth/*` token endpoints are mounted behind the **permissive**
/// (dev-mode) [`Authenticator`] (`REQ_0936`).
pub fn router(view: AppState, config: &GatewayConfig) -> Router {
    router_with_state(
        ServerState::detached(view),
        config,
        Arc::new(PermissiveAuthenticator),
    )
}

/// Build the axum application with a caller-supplied [`Authenticator`] behind
/// the seam (`REQ_0937`, `BB_0112`).
///
/// Like [`router`] but substitutes `authenticator` for the permissive default,
/// without touching any handler. Resource routes run enforcement = none
/// (`REQ_0938`): a `Bearer` token is accepted and never verified, and requests
/// with or without one always pass.
pub fn router_with_authenticator(
    view: AppState,
    config: &GatewayConfig,
    authenticator: Arc<dyn Authenticator>,
) -> Router {
    router_with_state(ServerState::detached(view), config, authenticator)
}

/// Build the application with a caller-supplied [`ActionSink`] behind the write
/// seam (`REQ_0969`).
///
/// Like [`router`] but substitutes `actions` for the default empty
/// [`SimActionSink`](taktora_medkit_provider::SimActionSink), so the operations
/// surface is backed by a configured simulation (tests, demos) or — later — a
/// real binding. The read core and auth are unchanged.
pub fn router_with_actions(
    view: AppState,
    config: &GatewayConfig,
    actions: Arc<dyn ActionSink>,
) -> Router {
    router_with_state(
        ServerState::detached_with_actions(view, actions),
        config,
        Arc::new(PermissiveAuthenticator),
    )
}

/// Build the application from a [`Gateway`], folding its provider snapshot into
/// the merged read-model once at construction.
///
/// When `config` carries a [`Manifest`](taktora_medkit_gateway::Manifest), the
/// snapshot is folded through it — declared Areas/Components become entities and
/// the raw provider entities are re-parented under them (`REQ_0921`); otherwise
/// the gateway's own grouping (flat, or a manifest attached to the gateway) is
/// used.
pub fn router_from_gateway<P: Provider>(gateway: &Gateway<P>, config: &GatewayConfig) -> Router {
    let view = config.manifest.as_ref().map_or_else(
        || gateway.view(),
        |manifest| {
            MergedView::from_snapshot_with_manifest(
                gateway.provider().snapshot(),
                Some(manifest.clone()),
            )
        },
    );
    router(Arc::new(view), config)
}

/// Serve the read-core over HTTP per `config`, backed by `view`, until the
/// process is shut down.
///
/// # Errors
///
/// Returns the underlying I/O error if the listener cannot bind, or — when a
/// [`TlsConfig`] is set but the `tls` feature is not enabled —
/// an `Unsupported` error.
pub async fn serve(config: GatewayConfig, view: AppState) -> std::io::Result<()> {
    let app = router(view, &config);
    if config.tls.is_some() {
        return serve_tls(config, app).await;
    }
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    axum::serve(listener, app).await
}

#[cfg(feature = "tls")]
async fn serve_tls(config: GatewayConfig, app: Router) -> std::io::Result<()> {
    let tls = config.tls.expect("tls config present");
    let rustls =
        axum_server::tls_rustls::RustlsConfig::from_pem_file(tls.cert_path, tls.key_path).await?;
    axum_server::bind_rustls(config.bind, rustls)
        .serve(app.into_make_service())
        .await
}

#[cfg(not(feature = "tls"))]
#[allow(clippy::unused_async)]
async fn serve_tls(_config: GatewayConfig, _app: Router) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "TLS requested but the `tls` feature is not enabled",
    ))
}

/// Serve the read-core on an already-bound `listener` (plaintext) until the
/// process is shut down.
///
/// Lets a caller bind the port itself — e.g. `127.0.0.1:0` for a race-free
/// ephemeral port in tests — and hand the listener to the server.
///
/// # Errors
///
/// Returns the underlying I/O error if the server loop fails.
pub async fn serve_listener(
    listener: tokio::net::TcpListener,
    view: AppState,
    config: &GatewayConfig,
) -> std::io::Result<()> {
    axum::serve(listener, router(view, config)).await
}

/// Serve the read-core plus the operations write surface backed by `actions` on
/// an already-bound `listener` (`REQ_0969`).
///
/// Like [`serve_listener`] but substitutes a configured [`ActionSink`] for the
/// default empty simulation.
///
/// # Errors
///
/// Returns the underlying I/O error if the server loop fails.
pub async fn serve_listener_with_actions(
    listener: tokio::net::TcpListener,
    view: AppState,
    config: &GatewayConfig,
    actions: Arc<dyn ActionSink>,
) -> std::io::Result<()> {
    axum::serve(listener, router_with_actions(view, config, actions)).await
}

/// Serve the read-core plus the **live** triggers + SSE surface on an
/// already-bound `listener`, polling `provider` every `cadence`.
///
/// Spawns the refresh-and-diff loop on the tokio side: it re-polls and re-merges
/// the provider snapshot, hot-swaps the
/// served read-model, and broadcasts the diff to `/api/v1/triggers/events` as
/// `fault_raised` / `fault_cleared` / `health_changed` events (`REQ_0930`–
/// `REQ_0934`). The loop runs off the control path; the served `MergedView` is
/// rebuilt under `config.manifest` when one is set (`REQ_0921`).
///
/// # Errors
///
/// Returns the underlying I/O error if the server loop fails.
pub async fn serve_listener_with_provider<P: Provider + Send + 'static>(
    listener: tokio::net::TcpListener,
    provider: P,
    config: &GatewayConfig,
    cadence: Duration,
) -> std::io::Result<()> {
    let manifest = config.manifest.clone();
    let initial = Arc::new(MergedView::from_snapshot_with_manifest(
        provider.snapshot(),
        manifest.clone(),
    ));
    let (state, view_tx, events, ring) = triggers::live_state(Arc::clone(&initial));
    tokio::spawn(triggers::refresh_loop(
        provider, manifest, initial, view_tx, events, ring, cadence,
    ));
    axum::serve(
        listener,
        router_with_state(state, config, Arc::new(PermissiveAuthenticator)),
    )
    .await
}

/// Bind an ephemeral port and return the listener plus the assigned address.
///
/// Convenience for tests and demos that must hit a live server without racing a
/// fixed port: bind `127.0.0.1:0` and read back the real port.
///
/// # Errors
///
/// Returns the bind error if `127.0.0.1:0` cannot be bound.
pub async fn ephemeral_listener() -> std::io::Result<(tokio::net::TcpListener, SocketAddr)> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_medkit_provider::MockProvider;

    #[test]
    fn router_builds_over_empty_provider() {
        let gateway = Gateway::new(MockProvider::new());
        let _app = router_from_gateway(&gateway, &GatewayConfig::default());
    }

    #[test]
    fn default_config_is_loopback_8080() {
        let config = GatewayConfig::default();
        assert_eq!(config.bind, DEFAULT_BIND);
        assert_eq!(config.bind.port(), 8080);
        assert!(config.cors.enabled);
        assert!(config.rate_limit.is_none());
        assert!(config.tls.is_none());
    }

    #[test]
    fn unknown_family_infers_unknown() {
        assert_eq!(
            infer_family("/api/v1/components/x/operations"),
            "operations"
        );
        assert_eq!(infer_family("/api/v1/components/x/bulk-data"), "bulk-data");
        assert_eq!(infer_family("/api/v1/wat"), "unknown");
    }
}
