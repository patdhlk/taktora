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

mod config;
pub mod demo;
mod error;
mod ratelimit;
mod triggers;

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
use taktora_medkit_provider::{Provider, Relation};
use tower_http::cors::{Any, CorsLayer};

use triggers::ServerState;

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
            get(|State(view): State<AppState>| async move { Json(view.health_document()) }),
        )
        .route(
            &format!("{API_BASE}/faults"),
            get(|State(view): State<AppState>, uri: Uri| async move {
                let filter = status_filter(&uri)?;
                Ok::<_, ApiError>(ok(&view.global_faults(filter)))
            }),
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

/// Apply the configured transport-hardening layers over a state-bound router.
fn router_with_state(state: ServerState, config: &GatewayConfig) -> Router {
    let mut app = api_router().with_state(state);

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
pub fn router(view: AppState, config: &GatewayConfig) -> Router {
    router_with_state(ServerState::detached(view), config)
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

/// Serve the read-core plus the **live** triggers + SSE surface on an
/// already-bound `listener`, polling `provider` every `cadence`.
///
/// Spawns the refresh-and-diff loop ([`refresh_loop`](triggers::refresh_loop)) on
/// the tokio side: it re-polls and re-merges the provider snapshot, hot-swaps the
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
    let (state, view_tx, events) = triggers::live_state(Arc::clone(&initial));
    tokio::spawn(triggers::refresh_loop(
        provider, manifest, initial, view_tx, events, cadence,
    ));
    axum::serve(listener, router_with_state(state, config)).await
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
