//! `TEST_0912` — the binding satisfies the medkit provider seam: a freshly
//! constructed binding exposes the synthetic executor entity plus one App per
//! registered task, all healthy, so the gateway can read through it
//! (`REQ_0923`, `REQ_0924`).

use taktora_medkit_binding_executor::ExecutorBinding;
use taktora_medkit_model::{EntityKind, Health};
use taktora_medkit_provider::Provider;

// @need-ids: TEST_0912
#[test]
fn binding_satisfies_provider_seam() {
    let binding = ExecutorBinding::with_tasks(["ctrl", "io"]);

    let entities = binding.entities();
    // The executor entity plus one App per registered task.
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].id, "executor");
    assert_eq!(entities[0].kind, EntityKind::Component);
    assert!(entities.iter().any(|e| e.id == "app:ctrl"));
    assert!(entities.iter().any(|e| e.id == "app:io"));
    assert!(
        entities
            .iter()
            .filter(|e| e.kind == EntityKind::App)
            .count()
            == 2
    );

    // No lifecycle observed yet: everything reads healthy, nothing faults.
    assert_eq!(binding.health("executor"), Health::Ok);
    assert_eq!(binding.health("app:ctrl"), Health::Ok);
    assert!(binding.faults("app:ctrl").is_empty());
    assert_eq!(binding.health("unknown"), Health::Ok);

    // An empty binding still surfaces the executor entity.
    assert_eq!(ExecutorBinding::new().entities().len(), 1);
}
