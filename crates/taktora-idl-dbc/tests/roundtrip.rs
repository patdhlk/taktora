//! End-to-end test: parse a small DBC, lower it, and check both the logical
//! module and the physical layout sidecar.

use taktora_idl_core::{Scalar, Type};
use taktora_idl_dbc::{ByteOrder, Multiplexer, lower, parse};

const SAMPLE: &str = r#"
VERSION "1.0"

BU_: ECU Dashboard Logger

BO_ 256 EngineData: 8 ECU
 SG_ Rpm : 0|16@1+ (0.25,0) [0|16383.75] "rpm" Dashboard,Logger
 SG_ CoolantTemp : 16|8@1- (1,-40) [-40|215] "degC" Dashboard
 SG_ Gear : 24|4@1+ (1,0) [0|7] "" Dashboard

BO_ 512 BodyControl: 2 Dashboard
 SG_ Mux M : 0|8@1+ (1,0) [0|255] "" ECU
 SG_ DoorState m0 : 8|4@1+ (1,0) [0|15] "" ECU

VAL_ 256 Gear 0 "Neutral" 1 "First" 2 "Second" ;
"#;

#[test]
fn parses_structure() {
    let db = parse(SAMPLE).unwrap();
    assert_eq!(db.version.as_deref(), Some("1.0"));
    assert_eq!(db.nodes, ["ECU", "Dashboard", "Logger"]);
    assert_eq!(db.messages.len(), 2);

    let engine = &db.messages[0];
    assert_eq!(engine.id, 256);
    assert_eq!(engine.name, "EngineData");
    assert_eq!(engine.dlc, 8);
    assert_eq!(engine.transmitter, "ECU");
    assert_eq!(engine.signals.len(), 3);

    let rpm = &engine.signals[0];
    assert_eq!(rpm.start_bit, 0);
    assert_eq!(rpm.bit_len, 16);
    assert_eq!(rpm.byte_order, ByteOrder::LittleEndian);
    assert!(!rpm.signed);
    assert!((rpm.factor - 0.25).abs() < f64::EPSILON);
    assert_eq!(rpm.unit, "rpm");
    assert_eq!(rpm.receivers, ["Dashboard", "Logger"]);

    let temp = &engine.signals[1];
    assert!(temp.signed);
    assert!((temp.offset - (-40.0)).abs() < f64::EPSILON);
    assert_eq!(temp.unit, "degC");
}

#[test]
fn parses_multiplexer_roles() {
    let db = parse(SAMPLE).unwrap();
    let body = &db.messages[1];
    assert_eq!(body.signals[0].multiplexer, Multiplexer::Multiplexor);
    assert_eq!(body.signals[1].multiplexer, Multiplexer::Multiplexed(0));
}

#[test]
fn parses_value_table() {
    let db = parse(SAMPLE).unwrap();
    let table = db.value_table_for(256, "Gear").unwrap();
    assert_eq!(
        table.entries,
        [
            (0, "Neutral".to_owned()),
            (1, "First".to_owned()),
            (2, "Second".to_owned()),
        ]
    );
}

#[test]
fn lowers_messages_to_bounded_structs() {
    let db = parse(SAMPLE).unwrap();
    let lowered = lower(&db, "vehicle").unwrap();
    let m = &lowered.module;

    // Module validates => every type has a finite serialized length.
    m.validate().unwrap();

    let engine = m.struct_by_name("EngineData").unwrap();
    assert_eq!(engine.fields.len(), 3);

    // 16-bit unsigned -> U16
    assert_eq!(engine.fields[0].ty, Type::Scalar(Scalar::U16));
    // 8-bit signed -> I8
    assert_eq!(engine.fields[1].ty, Type::Scalar(Scalar::I8));
    // Gear has a value table -> enum
    assert_eq!(engine.fields[2].ty, Type::Enum("EngineData_Gear".into()));

    let gear_enum = m.enum_by_name("EngineData_Gear").unwrap();
    assert_eq!(gear_enum.underlying, Scalar::U8); // 4 bits -> U8
    assert_eq!(gear_enum.variants.len(), 3);

    // Upper-bound size: U16 (2) + I8 (1) + enum U8 (1) = 4 bytes.
    assert_eq!(m.struct_max_serialized_len(engine).unwrap(), 4);
}

#[test]
fn layout_sidecar_carries_bit_packing() {
    let db = parse(SAMPLE).unwrap();
    let lowered = lower(&db, "vehicle").unwrap();

    let frame = lowered.layout.frame("EngineData").unwrap();
    assert_eq!(frame.can_id, 256);
    assert!(!frame.extended);
    assert_eq!(frame.dlc, 8);

    let rpm = &frame.signals[0];
    assert_eq!(rpm.start_bit, 0);
    assert_eq!(rpm.bit_len, 16);
    assert_eq!(rpm.byte_order, ByteOrder::LittleEndian);
    assert!((rpm.factor - 0.25).abs() < f64::EPSILON);
    assert_eq!(rpm.unit, "rpm");

    // The physical scaling lives here, not in the logical module.
    let temp = &frame.signals[1];
    assert!((temp.offset - (-40.0)).abs() < f64::EPSILON);
}

#[test]
fn extended_identifier_flag_is_decoded() {
    // 0x8000_0100 = extended flag (bit 31) set, CAN id 0x100.
    let dbc = "BO_ 2147483904 Diag: 8 ECU\n SG_ Code : 0|32@1+ (1,0) [0|0] \"\" Tester\n";
    let db = parse(dbc).unwrap();
    let msg = &db.messages[0];
    assert!(msg.is_extended());
    assert_eq!(msg.can_id(), 0x100);
}
