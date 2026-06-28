//! Run the `taktora-medkit` walking-skeleton gateway over the in-process mock
//! provider.
//!
//! ```text
//! cargo run -p taktora-medkit-gateway-axum --example serve
//! # → read-core SOVD API live at http://127.0.0.1:8080/api/v1/
//! ```
//!
//! Serves the read-diagnostic core (entity tree, relationships, faults, data)
//! and answers `501 Not Implemented` for the deferred families. The bind
//! address and the CORS / rate-limit / TLS knobs default per
//! [`GatewayConfig`](taktora_medkit_gateway_axum::GatewayConfig); override them
//! in code to suit.

use std::sync::Arc;

use taktora_medkit_gateway::Gateway;
use taktora_medkit_gateway_axum::{GatewayConfig, demo, serve};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let gateway = Gateway::new(demo::provider());
    let config = GatewayConfig::default();
    let view = Arc::new(gateway.view());

    println!(
        "taktora-medkit read-core SOVD API live at http://{}/api/v1/",
        config.bind
    );
    serve(config, view).await
}
