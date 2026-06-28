//! taktora-executor binding for `taktora-medkit`.
//!
//! This crate is one of the two seams where taktora coupling is quarantined
//! (`ADR_0111`). It sources liveness and timing from taktora-executor
//! [`Observer`](taktora_executor::Observer) /
//! [`ExecutionMonitor`](taktora_executor::ExecutionMonitor) hooks and feeds them
//! into the medkit [`Provider`] seam off the control path: the hooks only hand a
//! [`CycleObservation`] to a bounded
//! forwarding channel, never blocking or allocating on the `WaitSet` path
//! (`REQ_0910`, `REQ_0913`).
//!
//! Grounding scaffold: this establishes the crate, the dependency edge, and the
//! provider impl. The bounded-channel forwarding and the live
//! `Observer`/`ExecutionMonitor` wiring land in a downstream slice.

use taktora_executor::CycleObservation;
use taktora_medkit_model::{Dtc, Entity, Health};
use taktora_medkit_provider::Provider;

/// Binds taktora-executor observations into the medkit provider seam.
#[derive(Clone, Debug, Default)]
pub struct ExecutorBinding {
    _private: (),
}

impl ExecutorBinding {
    /// Create an executor binding.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Off-path hook called from the executor side for each completed cycle.
    ///
    /// A real implementation hands `observation` to a bounded forwarding channel
    /// and returns immediately (`REQ_0910`); this grounding stub is a no-op.
    pub const fn on_cycle(&self, _observation: &CycleObservation) {}
}

impl Provider for ExecutorBinding {
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
