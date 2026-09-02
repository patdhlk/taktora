//! `TEST_0915` (mapping subset) — connector health discriminators map to the
//! medkit worst-wins `Health` ladder, and a fresh `MedkitProvider` satisfies the
//! provider seam with its raw Component and no faults.

use taktora_connector_core::health::ConnectorHealthKind;
use taktora_medkit_binding_connector::{MedkitProvider, map_health};
use taktora_medkit_model::{EntityKind, Health};
use taktora_medkit_provider::Provider;

// @need-ids: TEST_0915
#[test]
fn health_kinds_map_to_worst_wins_ladder() {
    assert_eq!(map_health(ConnectorHealthKind::Up), Health::Ok);
    assert_eq!(map_health(ConnectorHealthKind::Connecting), Health::Warning);
    assert_eq!(map_health(ConnectorHealthKind::Degraded), Health::Warning);
    assert_eq!(map_health(ConnectorHealthKind::Down), Health::Error);
}

// @need-ids: TEST_0915
#[test]
fn fresh_binding_emits_raw_component_and_no_faults() {
    let binding = MedkitProvider::new("component:ethercat0", "EtherCAT bus 0");

    let entities = binding.entities();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id, "component:ethercat0");
    assert_eq!(entities[0].kind, EntityKind::Component);
    // Emitted raw — no placement until the manifest (#82) provides one.
    assert!(entities[0].parent_id.is_none());

    assert!(binding.faults("component:ethercat0").is_empty());
    assert!(binding.faults("something-else").is_empty());
    assert_eq!(binding.health("component:ethercat0"), Health::Ok);
}
