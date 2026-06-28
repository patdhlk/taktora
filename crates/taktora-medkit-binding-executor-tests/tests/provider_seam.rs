//! Grounding smoke test — `ExecutorBinding` satisfies the medkit provider seam
//! so the gateway can read through it once the off-path forwarding lands
//! (`REQ_0910`, `REQ_0913`).

use taktora_medkit_binding_executor::ExecutorBinding;
use taktora_medkit_model::Health;
use taktora_medkit_provider::Provider;

#[test]
fn binding_satisfies_provider_seam() {
    let binding = ExecutorBinding::new();
    assert!(binding.entities().is_empty());
    assert!(binding.faults("anything").is_empty());
    assert_eq!(binding.health("anything"), Health::Ok);
}
