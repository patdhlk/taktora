//! Golden-fixture test: the canonical wire example other languages target.
//!
//! `tests/golden_manifest.json` is the checked-in JSON contract example. The
//! `golden_manifest_matches_checked_in_fixture` test asserts our serialization
//! still produces it. To regenerate after an intentional contract change, run:
//!
//! ```text
//! cargo test -p taktora-connector-ui-contract --test golden -- --ignored regenerate
//! ```

use std::path::PathBuf;

use taktora_connector_ui_contract::{
    CommandSchema, FieldSchema, FieldType, Kind, Manifest, ViewModelSchema, contract_hash,
};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden_manifest.json")
}

/// Build the representative manifest with its computed structural hash.
fn representative_manifest() -> Manifest {
    let mut m = Manifest {
        instance: "ethercat-stepper".into(),
        epoch: 1,
        contract_hash: String::new(),
        view_models: vec![
            ViewModelSchema {
                name: "System".into(),
                service: "ethercat-stepper/vm/System".into(),
                fields: vec![
                    FieldSchema {
                        name: "counter".into(),
                        ty: FieldType::U64,
                    },
                    FieldSchema {
                        name: "epoch".into(),
                        ty: FieldType::U64,
                    },
                ],
            },
            ViewModelSchema {
                name: "Stepper".into(),
                service: "ethercat-stepper/vm/Stepper".into(),
                fields: vec![
                    FieldSchema {
                        name: "position".into(),
                        ty: FieldType::F64,
                    },
                    FieldSchema {
                        name: "state".into(),
                        ty: FieldType::Enum {
                            name: "StepperState".into(),
                            variants: vec![
                                ("Idle".into(), 0),
                                ("Running".into(), 1),
                                ("Faulted".into(), 2),
                            ],
                            width: 1,
                        },
                    },
                    FieldSchema {
                        name: "can_jog".into(),
                        ty: FieldType::Bool,
                    },
                    FieldSchema {
                        name: "label".into(),
                        ty: FieldType::Str { cap: 16 },
                    },
                    FieldSchema {
                        name: "history".into(),
                        ty: FieldType::Array {
                            elem: Box::new(FieldType::F64),
                            len: 4,
                        },
                    },
                ],
            },
        ],
        commands: vec![
            CommandSchema {
                name: "enable".into(),
                request_service: "ethercat-stepper/cmd/enable/req".into(),
                reply_service: "ethercat-stepper/cmd/enable/rep".into(),
                params: vec![FieldSchema {
                    name: "force".into(),
                    ty: FieldType::Bool,
                }],
                kind: Kind::Command,
                idempotent: true,
                can_execute_service: Some("ethercat-stepper/cmd/enable/can".into()),
            },
            CommandSchema {
                name: "jog_relative".into(),
                request_service: "ethercat-stepper/cmd/jog_relative/req".into(),
                reply_service: "ethercat-stepper/cmd/jog_relative/rep".into(),
                params: vec![FieldSchema {
                    name: "delta".into(),
                    ty: FieldType::F64,
                }],
                kind: Kind::Command,
                idempotent: false,
                can_execute_service: None,
            },
        ],
    };
    m.contract_hash = contract_hash(&m);
    m
}

#[test]
fn golden_manifest_matches_checked_in_fixture() {
    let expected = representative_manifest();
    let golden = std::fs::read_to_string(golden_path())
        .expect("tests/golden_manifest.json must exist (regenerate with the --ignored test)");

    // Whitespace-insensitive structural comparison of the JSON.
    let golden_value: serde_json::Value = serde_json::from_str(&golden).unwrap();
    let expected_value = serde_json::to_value(&expected).unwrap();
    assert_eq!(
        golden_value, expected_value,
        "serialized manifest drifted from the golden fixture"
    );

    // The golden file must also deserialize back to the same manifest.
    let parsed: Manifest = serde_json::from_str(&golden).unwrap();
    assert_eq!(parsed, expected);
}

#[test]
#[ignore = "regenerates the golden fixture on demand"]
fn regenerate() {
    let m = representative_manifest();
    let json = serde_json::to_string_pretty(&m).unwrap();
    std::fs::write(golden_path(), format!("{json}\n")).unwrap();
}
