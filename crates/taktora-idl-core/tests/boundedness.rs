//! Integration tests for the boundedness contract of [`taktora_idl_core`].

use taktora_idl_core::{
    EnumDef, EnumVariant, Field, IrError, LENGTH_PREFIX_BYTES, Module, Scalar, Service, Struct,
    Type,
};

fn struct_with(name: &str, fields: Vec<Field>) -> Struct {
    Struct {
        name: name.to_owned(),
        fields,
    }
}

#[test]
fn scalar_sizes_are_fixed() {
    assert_eq!(Scalar::Bool.wire_size(), 1);
    assert_eq!(Scalar::U16.wire_size(), 2);
    assert_eq!(Scalar::I32.wire_size(), 4);
    assert_eq!(Scalar::F64.wire_size(), 8);
}

#[test]
fn integer_for_bits_picks_narrowest_power_of_two() {
    assert_eq!(Scalar::integer_for_bits(1, false), Some(Scalar::U8));
    assert_eq!(Scalar::integer_for_bits(8, true), Some(Scalar::I8));
    assert_eq!(Scalar::integer_for_bits(11, false), Some(Scalar::U16));
    assert_eq!(Scalar::integer_for_bits(40, true), Some(Scalar::I64));
    assert_eq!(Scalar::integer_for_bits(0, false), None);
    assert_eq!(Scalar::integer_for_bits(65, false), None);
}

#[test]
fn flat_struct_sums_field_sizes() {
    let m = Module {
        name: "m".into(),
        structs: vec![struct_with(
            "EngineData",
            vec![
                Field::new("rpm", Type::scalar(Scalar::U16)),
                Field::new("temp", Type::scalar(Scalar::I8)),
                Field::new("throttle", Type::scalar(Scalar::U8)),
            ],
        )],
        ..Module::default()
    };
    m.validate().unwrap();
    let s = m.struct_by_name("EngineData").unwrap();
    assert_eq!(m.struct_max_serialized_len(s).unwrap(), 2 + 1 + 1);
}

#[test]
fn bounded_string_and_sequence_have_finite_upper_bound() {
    let m = Module {
        name: "m".into(),
        structs: vec![struct_with(
            "Frame",
            vec![
                Field::new("label", Type::String { capacity: 16 }),
                Field::new("samples", Type::sequence(Type::scalar(Scalar::U32), 8)),
            ],
        )],
        ..Module::default()
    };
    m.validate().unwrap();
    let s = m.struct_by_name("Frame").unwrap();
    let expected = (LENGTH_PREFIX_BYTES + 16) + (LENGTH_PREFIX_BYTES + 8 * 4);
    assert_eq!(m.struct_max_serialized_len(s).unwrap(), expected);
}

#[test]
fn nested_struct_and_enum_resolve_and_size() {
    let m = Module {
        name: "m".into(),
        structs: vec![
            struct_with("Inner", vec![Field::new("v", Type::scalar(Scalar::U32))]),
            struct_with(
                "Outer",
                vec![
                    Field::new("gear", Type::Enum("Gear".into())),
                    Field::new("inner", Type::Struct("Inner".into())),
                ],
            ),
        ],
        enums: vec![EnumDef {
            name: "Gear".into(),
            underlying: Scalar::U8,
            variants: vec![
                EnumVariant {
                    name: "Neutral".into(),
                    value: 0,
                },
                EnumVariant {
                    name: "First".into(),
                    value: 1,
                },
            ],
        }],
        ..Module::default()
    };
    m.validate().unwrap();
    let outer = m.struct_by_name("Outer").unwrap();
    // enum U8 (1) + Inner { U32 } (4)
    assert_eq!(m.struct_max_serialized_len(outer).unwrap(), 1 + 4);
}

#[test]
fn unknown_type_reference_is_rejected() {
    let m = Module {
        name: "m".into(),
        structs: vec![struct_with(
            "Outer",
            vec![Field::new("missing", Type::Struct("Ghost".into()))],
        )],
        ..Module::default()
    };
    let err = m.validate().unwrap_err();
    assert!(matches!(err, IrError::UnknownType { name, .. } if name == "Ghost"));
}

#[test]
fn recursive_struct_is_rejected() {
    let m = Module {
        name: "m".into(),
        structs: vec![struct_with(
            "Node",
            vec![Field::new("next", Type::Struct("Node".into()))],
        )],
        ..Module::default()
    };
    let err = m.validate().unwrap_err();
    assert!(matches!(err, IrError::RecursiveType { .. }));
}

#[test]
fn duplicate_type_name_is_rejected() {
    let m = Module {
        name: "m".into(),
        structs: vec![
            struct_with("Dup", vec![]),
            struct_with("Dup", vec![Field::new("x", Type::scalar(Scalar::U8))]),
        ],
        ..Module::default()
    };
    let err = m.validate().unwrap_err();
    assert!(matches!(err, IrError::DuplicateType { name } if name == "Dup"));
}

#[test]
fn service_payload_references_are_validated() {
    let m = Module {
        name: "m".into(),
        structs: vec![struct_with(
            "Ping",
            vec![Field::new("seq", Type::scalar(Scalar::U32))],
        )],
        services: vec![Service {
            name: "Echo".into(),
            request: "Ping".into(),
            response: Some("Pong".into()), // undefined
        }],
        ..Module::default()
    };
    let err = m.validate().unwrap_err();
    assert!(
        matches!(err, IrError::UnknownType { referrer, name } if referrer == "Echo" && name == "Pong")
    );
}
