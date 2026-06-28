//! Connector-framework binding for `taktora-medkit`.
//!
//! This crate is one of the two seams where taktora coupling is quarantined
//! (`ADR_0111`). It maps connector-framework
//! [`ConnectorHealthKind`]
//! transitions into the medkit [`Health`] / SOVD Component model and feeds the
//! [`Provider`] seam off the control path (`REQ_0910`, `REQ_0912`).
//!
//! Grounding scaffold: the health mapping below is real and tested; the
//! bounded-channel forwarding, Component synthesis, and last-sample
//! freeze-frames land in a downstream slice.

use taktora_connector_core::health::ConnectorHealthKind;
use taktora_medkit_model::{Dtc, Entity, Health};
use taktora_medkit_provider::Provider;

/// Map a connector health discriminator to the medkit [`Health`] it implies for
/// the Component standing in for that connector (`REQ_0912`).
#[must_use]
pub const fn map_health(kind: ConnectorHealthKind) -> Health {
    match kind {
        ConnectorHealthKind::Up => Health::Ok,
        // A connector that is reconnecting or degraded is a warning, not yet a
        // hard fault: the bus may recover without intervention.
        ConnectorHealthKind::Connecting | ConnectorHealthKind::Degraded => Health::Warning,
        ConnectorHealthKind::Down => Health::Error,
    }
}

/// Binds connector-framework health transitions into the medkit provider seam.
#[derive(Clone, Debug, Default)]
pub struct ConnectorBinding {
    _private: (),
}

impl ConnectorBinding {
    /// Create a connector binding.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Provider for ConnectorBinding {
    fn entities(&self) -> Vec<Entity> {
        Vec::new()
    }

    fn faults(&self, _entity_id: &str) -> Vec<Dtc> {
        Vec::new()
    }

    fn health(&self, _entity_id: &str) -> Health {
        Health::Ok
    }
}
