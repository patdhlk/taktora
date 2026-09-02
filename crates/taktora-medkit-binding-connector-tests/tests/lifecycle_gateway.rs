//! `TEST_0915` / `TEST_0916` — a simulated `Up → Degraded → Down → Up` health
//! sequence, fed into a `MedkitProvider` and read back through the
//! transport-neutral `taktora-medkit-gateway` (the running gateway core),
//! produces the expected DTC lifecycle and worst-wins Component health, and the
//! occurrence count increments across repeated transitions.

use std::time::Instant;

use taktora_connector_core::health::ConnectorHealth;
use taktora_medkit_binding_connector::{DTC_DEGRADED, DTC_NOT_OPERATIONAL, MedkitProvider};
use taktora_medkit_gateway::Gateway;
use taktora_medkit_model::{EntityKind, Health, Severity};

const COMPONENT: &str = "component:ethercat0";

fn degraded(reason: &str) -> ConnectorHealth {
    ConnectorHealth::Degraded {
        reason: reason.to_owned(),
    }
}

fn down(reason: &str) -> ConnectorHealth {
    ConnectorHealth::Down {
        reason: reason.to_owned(),
        since: Instant::now(),
    }
}

/// Find the fault with `code` in the entity-scoped fault list served by the
/// gateway view.
fn fault(
    view: &taktora_medkit_gateway::MergedView,
    code: &str,
) -> Option<taktora_medkit_model::FaultSummary> {
    view.entity_faults(
        EntityKind::Component,
        COMPONENT,
        taktora_medkit_gateway::FaultStatusFilter::All,
    )
    .ok()?
    .items
    .into_iter()
    .find(|f| f.fault_code == code)
}

// @need-ids: TEST_0915, TEST_0916
#[test]
fn up_degraded_down_up_drives_dtc_lifecycle_through_gateway() {
    let binding = MedkitProvider::new(COMPONENT, "EtherCAT bus 0");
    // The gateway reads through a clone that shares the binding's store.
    let gateway = Gateway::new(binding.clone());

    // --- Up (nominal): the Component is present and healthy, no DTC. ----------
    binding.apply(&ConnectorHealth::Up, 1.0);
    assert_eq!(gateway.entities().items.len(), 1);
    assert_eq!(gateway.entities().items[0].kind, EntityKind::Component);
    assert_eq!(gateway.rolled_up_health(COMPONENT), Health::Ok);
    assert!(gateway.faults(COMPONENT).items.is_empty());

    // --- Degraded: a Warning DTC carrying the reason, Component Warning. ------
    binding.apply(&degraded("working counter below expected"), 2.0);
    assert_eq!(gateway.rolled_up_health(COMPONENT), Health::Warning);
    let view = gateway.view();
    let deg = fault(&view, DTC_DEGRADED).expect("degraded DTC raised");
    assert_eq!(deg.severity, Severity::Warn.wire_value());
    assert_eq!(deg.status, "CONFIRMED");
    assert!(deg.description.contains("working counter below expected"));
    assert_eq!(deg.occurrence_count, 1);

    // --- Down: a Critical DTC, the degraded DTC superseded (healed), Component
    //     rolls up to Critical. ------------------------------------------------
    binding.apply(&down("link lost"), 3.0);
    assert_eq!(gateway.rolled_up_health(COMPONENT), Health::Critical);
    let view = gateway.view();
    let crit = fault(&view, DTC_NOT_OPERATIONAL).expect("not-operational DTC raised");
    assert_eq!(crit.severity, Severity::Critical.wire_value());
    assert_eq!(crit.status, "CONFIRMED");
    assert_eq!(crit.occurrence_count, 1);
    // The degraded condition is no longer active.
    assert_eq!(fault(&view, DTC_DEGRADED).unwrap().status, "HEALED");

    // --- Up again: every DTC heals, Component returns to Ok, but the DTCs stay
    //     in memory (confirmed) for the maintenance history. -------------------
    binding.apply(&ConnectorHealth::Up, 4.0);
    assert_eq!(gateway.rolled_up_health(COMPONENT), Health::Ok);
    let view = gateway.view();
    assert_eq!(fault(&view, DTC_NOT_OPERATIONAL).unwrap().status, "HEALED");
    assert_eq!(fault(&view, DTC_DEGRADED).unwrap().status, "HEALED");
}

// @need-ids: TEST_0915, TEST_0916
#[test]
fn repeated_degraded_increments_occurrence_count_through_gateway() {
    let binding = MedkitProvider::new(COMPONENT, "EtherCAT bus 0");
    let gateway = Gateway::new(binding.clone());

    // Three separate degraded episodes, each cleared by a return to Up.
    binding.apply(&degraded("first"), 1.0);
    binding.apply(&ConnectorHealth::Up, 2.0);
    binding.apply(&degraded("second"), 3.0);
    binding.apply(&ConnectorHealth::Up, 4.0);
    binding.apply(&degraded("third"), 5.0);

    let view = gateway.view();
    let deg = fault(&view, DTC_DEGRADED).expect("degraded DTC");
    assert_eq!(deg.occurrence_count, 3);
    // First/last occurrence bracket the whole episode history.
    assert!(deg.first_occurred < deg.last_occurred);
    assert_eq!(gateway.rolled_up_health(COMPONENT), Health::Warning);
}
