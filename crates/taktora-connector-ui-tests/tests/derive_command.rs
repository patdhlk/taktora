//! Integration tests for `#[derive(CommandParams)]` and `#[command(idempotent)]`.

use serde::Serialize;
use taktora_connector_ui::{CommandParams, ImageEnum};
use taktora_connector_ui_contract::{FieldSchema, FieldType, Kind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ImageEnum)]
#[repr(u8)]
enum Direction {
    Forward = 0,
    Reverse = 1,
}

#[derive(Serialize, CommandParams)]
#[command(idempotent)]
struct Enable {
    force: bool,
}

#[derive(Serialize, CommandParams)]
struct JogRelative {
    distance: f64,
    direction: Direction,
}

#[derive(Serialize, CommandParams)]
struct NoParams;

#[test]
fn idempotent_attribute_sets_the_flag() {
    // Bind through locals so clippy treats these as runtime values, not
    // const-folded assertions.
    let enable_idem = Enable::IDEMPOTENT;
    let jog_idem = JogRelative::IDEMPOTENT;
    assert!(enable_idem);
    assert!(!jog_idem);
}

#[test]
fn params_lowers_fields_to_schema() {
    assert_eq!(
        Enable::params(),
        vec![FieldSchema {
            name: "force".to_owned(),
            ty: FieldType::Bool,
        }]
    );
    assert_eq!(
        JogRelative::params(),
        vec![
            FieldSchema {
                name: "distance".to_owned(),
                ty: FieldType::F64,
            },
            FieldSchema {
                name: "direction".to_owned(),
                ty: FieldType::Enum {
                    name: "Direction".to_owned(),
                    variants: vec![("Forward".to_owned(), 0), ("Reverse".to_owned(), 1)],
                    width: 1,
                },
            },
        ]
    );
}

#[test]
fn unit_struct_has_no_params() {
    assert!(NoParams::params().is_empty());
    let noparams_idem = NoParams::IDEMPOTENT;
    assert!(!noparams_idem);
}

#[test]
fn command_schema_contribution_carries_kind_and_idempotent() {
    let schema = Enable::command_schema(
        "enable",
        "inst/cmd/enable/req",
        "inst/cmd/enable/rep",
        Some("inst/cmd/enable/can".to_owned()),
    );
    assert_eq!(schema.name, "enable");
    assert_eq!(schema.request_service, "inst/cmd/enable/req");
    assert_eq!(schema.reply_service, "inst/cmd/enable/rep");
    assert_eq!(schema.kind, Kind::Command);
    assert!(schema.idempotent);
    assert_eq!(
        schema.can_execute_service.as_deref(),
        Some("inst/cmd/enable/can")
    );
    assert_eq!(schema.params, Enable::params());
}
