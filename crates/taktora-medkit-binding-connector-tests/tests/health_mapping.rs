//! TEST_0903 (grounding subset) — connector health discriminators map to the
//! medkit worst-wins `Health` ladder, and `ConnectorBinding` satisfies the
//! provider seam.

use taktora_connector_core::health::ConnectorHealthKind;
use taktora_medkit_binding_connector::{ConnectorBinding, map_health};
use taktora_medkit_model::Health;
use taktora_medkit_provider::Provider;

#[test]
fn health_kinds_map_to_worst_wins_ladder() {
    assert_eq!(map_health(ConnectorHealthKind::Up), Health::Ok);
    assert_eq!(map_health(ConnectorHealthKind::Connecting), Health::Warning);
    assert_eq!(map_health(ConnectorHealthKind::Degraded), Health::Warning);
    assert_eq!(map_health(ConnectorHealthKind::Down), Health::Error);
}

#[test]
fn binding_satisfies_provider_seam() {
    let binding = ConnectorBinding::new();
    assert!(binding.entities().is_empty());
    assert!(binding.faults("anything").is_empty());
    assert_eq!(binding.health("anything"), Health::Ok);
}
