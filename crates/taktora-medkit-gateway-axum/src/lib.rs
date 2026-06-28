//! axum HTTP surface for `taktora-medkit`.
//!
//! Thin adapter that exposes the transport-neutral [`Gateway`] over HTTP on a
//! tokio runtime, on the way to a REST surface drop-in compatible with the
//! `ros2_medkit` contract (`REQ_0911`). axum and tokio are not taktora
//! dependencies, so this crate stays part of the extractable core (`REQ_0916`,
//! `ADR_0111`).
//!
//! This grounding scaffold wires a single liveness route. The walking skeleton
//! that serves the SOVD read families (and `501 Not Implemented` for the
//! deferred ones) is a downstream slice.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use taktora_medkit_gateway::Gateway;
use taktora_medkit_provider::Provider;

/// Liveness handler: reports that the gateway is up and how many entities it
/// currently sees.
async fn health<P: Provider + 'static>(State(gateway): State<Arc<Gateway<P>>>) -> String {
    format!("ok entities={}", gateway.entities().x_medkit.total_count)
}

/// Build the axum router exposing `gateway` over HTTP.
///
/// The router carries the gateway as shared state; clone the [`Arc`] to keep a
/// handle for tests or shutdown coordination.
pub fn router<P: Provider + 'static>(gateway: Arc<Gateway<P>>) -> Router {
    Router::new()
        .route("/health", get(health::<P>))
        .with_state(gateway)
}

/// Serve the gateway over HTTP on `addr` until the process is shut down.
///
/// # Errors
///
/// Returns the underlying I/O error if the listener cannot bind `addr` or the
/// server loop fails.
pub async fn serve<P: Provider + 'static>(
    addr: SocketAddr,
    gateway: Arc<Gateway<P>>,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(gateway)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_medkit_provider::MockProvider;

    /// The router wires the liveness handler over the gateway state without an
    /// HTTP framework error. (The HTTP-level walking-skeleton tests land with
    /// the read families in a downstream slice.)
    #[test]
    fn router_builds_over_gateway() {
        let gateway = Arc::new(Gateway::new(MockProvider::new()));
        assert_eq!(gateway.entities().x_medkit.total_count, 0);
        let _router = router(gateway);
    }
}
