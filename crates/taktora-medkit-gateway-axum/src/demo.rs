//! The walking-skeleton demo dataset.
//!
//! A self-consistent [`MockProvider`] seeded to resemble the captured
//! `ros2_medkit` corpus, so `cargo run --example serve` serves recognisable
//! SOVD bodies and the integration test can shape-diff live responses against
//! `contract/golden/*.json`. This is illustrative fixture data, not a live
//! source; real data arrives through the binding crates and the provider seam.

use serde_json::json;
use taktora_medkit_model::{Entity, EntityKind, EntityMeta, FaultSummary, Ros2Ref, Severity};
use taktora_medkit_provider::{MockProvider, Relation, Telemetry};

const COMPONENT: &str = "spark-6723";
const APPS: [&str; 4] = [
    "ros2_medkit_gateway",
    "ros2_medkit_gateway_fault_clients",
    "ros2_medkit_gateway_lifecycle_state_reader",
    "ros2_medkit_gateway_sub",
];

fn component_entity() -> Entity {
    Entity {
        href: format!("/api/v1/components/{COMPONENT}"),
        id: COMPONENT.to_owned(),
        name: COMPONENT.to_owned(),
        kind: EntityKind::Component,
        parent_id: None,
        description: Some("Ubuntu 24.04.4 LTS on x86_64".to_owned()),
        x_medkit: Some(EntityMeta {
            source: Some("runtime".to_owned()),
            ..EntityMeta::default()
        }),
    }
}

/// A bare component reference, as relationship sub-resources emit it (no
/// `x-medkit`), e.g. under `…/is-located-on`.
fn component_ref() -> Entity {
    Entity {
        href: format!("/api/v1/components/{COMPONENT}"),
        id: COMPONENT.to_owned(),
        name: COMPONENT.to_owned(),
        kind: EntityKind::Component,
        parent_id: None,
        description: None,
        x_medkit: None,
    }
}

fn function_entity() -> Entity {
    Entity {
        href: "/api/v1/functions/root".to_owned(),
        id: "root".to_owned(),
        name: "root".to_owned(),
        kind: EntityKind::Function,
        parent_id: None,
        description: None,
        x_medkit: Some(EntityMeta {
            source: Some("runtime".to_owned()),
            ..EntityMeta::default()
        }),
    }
}

/// A top-level `/apps` item: carries `component_id`.
fn app_list_item(id: &str) -> Entity {
    Entity {
        href: format!("/api/v1/apps/{id}"),
        id: id.to_owned(),
        name: id.to_owned(),
        kind: EntityKind::App,
        parent_id: Some(COMPONENT.to_owned()),
        description: None,
        x_medkit: Some(EntityMeta {
            component_id: Some(COMPONENT.to_owned()),
            is_online: Some(true),
            ros2: Some(Ros2Ref {
                node: format!("/{id}"),
            }),
            source: Some("heuristic".to_owned()),
        }),
    }
}

/// A relationship app item (e.g. under `…/hosts`): omits `component_id`.
fn app_rel_item(id: &str) -> Entity {
    Entity {
        href: format!("/api/v1/apps/{id}"),
        id: id.to_owned(),
        name: id.to_owned(),
        kind: EntityKind::App,
        parent_id: None,
        description: None,
        x_medkit: Some(EntityMeta {
            is_online: Some(true),
            ros2: Some(Ros2Ref {
                node: format!("/{id}"),
            }),
            source: Some("heuristic".to_owned()),
            ..EntityMeta::default()
        }),
    }
}

fn fault(code: &str, description: &str, severity: Severity, status: &str) -> FaultSummary {
    FaultSummary {
        description: description.to_owned(),
        fault_code: code.to_owned(),
        first_occurred: 1_782_600_000.25,
        last_occurred: 1_782_661_500.75,
        occurrence_count: 7,
        reporting_sources: vec!["/ros2_medkit_gateway".to_owned()],
        severity: severity.wire_value(),
        severity_label: format!("{severity:?}").to_uppercase(),
        status: status.to_owned(),
    }
}

/// Plausible non-zero `/health` telemetry for the walking skeleton, mirroring
/// the captured `contract/golden/health.json` so the live `/health` looks real
/// rather than all-zero (`REQ_0978`).
fn demo_telemetry() -> Telemetry {
    Telemetry {
        data_provider: [
            ("pool_cap".to_owned(), json!(256)),
            ("cold_wait_cap".to_owned(), json!(4)),
            ("pool_size".to_owned(), json!(5)),
            ("pool_hits".to_owned(), json!(128)),
            ("pool_misses".to_owned(), json!(5)),
            ("graph_events_received".to_owned(), json!(42)),
        ]
        .into_iter()
        .collect(),
        subscription_executor: [
            ("worker_alive".to_owned(), json!(true)),
            ("queue_depth".to_owned(), json!(0)),
            ("queue_max_depth_observed".to_owned(), json!(3)),
            ("tasks_completed".to_owned(), json!(1024)),
            ("last_task_latency_us".to_owned(), json!(180)),
            ("max_task_latency_us".to_owned(), json!(1500)),
            ("graph_events_received".to_owned(), json!(42)),
        ]
        .into_iter()
        .collect(),
        entity_cache: [
            ("generation".to_owned(), json!(3)),
            ("capacity".to_owned(), json!(256)),
            ("grew".to_owned(), json!(false)),
        ]
        .into_iter()
        .collect(),
    }
}

/// Build the seeded walking-skeleton provider.
#[must_use]
pub fn provider() -> MockProvider {
    let brake = fault(
        "BRAKE_PRESSURE_LOW",
        "Brake circuit pressure below safe threshold",
        Severity::Error,
        "CONFIRMED",
    );
    let motor = fault(
        "MOTOR_OVERHEAT",
        "Drive motor temperature trending high",
        Severity::Warn,
        "PREFAILED",
    );

    let mut provider = MockProvider::new()
        .with_entity(component_entity())
        .with_entity(function_entity())
        .with_fault(COMPONENT, brake.clone())
        .with_fault(COMPONENT, motor.clone())
        .with_fault("ros2_medkit_gateway", brake)
        .with_fault("ros2_medkit_gateway_sub", motor)
        .with_data(COMPONENT, json!({ "cpu": { "load_avg": 0.42 } }))
        .with_telemetry(demo_telemetry());

    for id in APPS {
        provider = provider
            .with_entity(app_list_item(id))
            .with_relationship(COMPONENT, Relation::Hosts, app_rel_item(id))
            .with_relationship("root", Relation::Hosts, app_rel_item(id))
            .with_relationship(id, Relation::IsLocatedOn, component_ref());
    }
    provider
}
