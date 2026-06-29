//! The **logs** family: SOVD per-entity diagnostic logs over HTTP (`REQ_0976`).
//!
//! Logs are mostly a **read** family — the entries come from the merged
//! [`MergedView`](taktora_medkit_gateway::MergedView) snapshot, exactly like
//! `…/data`, and `GET …/logs` filters them by an optional `?severity=` (exact)
//! and `?context=` (substring). A tiny configuration sub-resource rides the
//! **write** seam: `GET …/logs/configuration` reads the stored configuration and
//! `PUT …/logs/configuration` upserts it, both through the [`ActionSink`] facade
//! so the in-memory
//! [`SimActionSink`](taktora_medkit_provider::SimActionSink) backs them in tests
//! and the walking skeleton, and a real binding backs them later unchanged.
//!
//! [`ActionSink`]: taktora_medkit_provider::ActionSink

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::Uri;
use axum::routing::get;
use serde::Serialize;
use serde_json::Value;
use taktora_medkit_gateway::view::{API_BASE, collection_segment};
use taktora_medkit_model::EntityKind;
use taktora_medkit_provider::ResourceRef;

use crate::AppState;
use crate::error::ApiError;
use crate::triggers::ServerState;

/// A collection envelope (`items` + `x-medkit.total_count`) over any wire item.
#[derive(Serialize)]
struct Collection<T> {
    items: Vec<T>,
    #[serde(rename = "x-medkit")]
    x_medkit: CollectionMeta,
}

#[derive(Serialize)]
struct CollectionMeta {
    total_count: usize,
}

fn collection<T: Serialize>(items: Vec<T>) -> Json<Collection<T>> {
    let total_count = items.len();
    Json(Collection {
        items,
        x_medkit: CollectionMeta { total_count },
    })
}

/// Parse a single `?key=value` query parameter, mirroring `status_filter` in the
/// crate root. Returns `None` when the key is absent.
fn query_param(uri: &Uri, key: &str) -> Option<String> {
    uri.query().and_then(|query| {
        query.split('&').find_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            (parts.next()? == key).then(|| parts.next().unwrap_or("").to_owned())
        })
    })
}

/// The logs routes for one entity `kind` (`REQ_0976`): the read `…/logs` list
/// plus the `…/logs/configuration` GET/PUT that rides the write seam. Mounted on
/// all four entity kinds.
pub fn log_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}/{{id}}/logs", collection_segment(kind));
    let config = format!("{base}/configuration");
    Router::new()
        .route(
            &base,
            get(
                move |State(view): State<AppState>, Path(id): Path<String>, uri: Uri| async move {
                    let severity = query_param(&uri, "severity");
                    let context = query_param(&uri, "context");
                    view.logs(kind, &id, severity.as_deref(), context.as_deref())
                        .map(collection)
                        .map_err(ApiError::from)
                },
            ),
        )
        .route(
            &config,
            get(
                move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    Json(
                        state
                            .actions()
                            .log_configuration(&ResourceRef::new(kind, id)),
                    )
                },
            )
            .put(
                move |State(state): State<ServerState>,
                      Path(id): Path<String>,
                      body: Json<Value>| async move {
                    let Json(value) = body;
                    Json(
                        state
                            .actions()
                            .set_log_configuration(&ResourceRef::new(kind, id), value),
                    )
                },
            ),
        )
}
