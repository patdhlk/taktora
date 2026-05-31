//! Integration tests for [`taktora_fieldbus_od_core::DataType`].

use taktora_fieldbus_od_core::DataType;

#[test]
fn known_coe_typenames_parse_to_scalars() {
    assert_eq!(DataType::parse_coe_name("BOOL"), DataType::Bool);
    assert_eq!(DataType::parse_coe_name("USINT"), DataType::U8);
    assert_eq!(DataType::parse_coe_name("UINT"), DataType::U16);
    assert_eq!(DataType::parse_coe_name("UDINT"), DataType::U32);
    assert_eq!(DataType::parse_coe_name("INT"), DataType::I16);
    assert_eq!(DataType::parse_coe_name("DINT"), DataType::I32);
    assert_eq!(DataType::parse_coe_name("REAL"), DataType::Real32);
    assert_eq!(
        DataType::parse_coe_name("STRING(8)"),
        DataType::VisibleString
    );
}

#[test]
fn bit_widths_parse_to_bitn() {
    assert_eq!(DataType::parse_coe_name("BIT1"), DataType::BitN(1));
    assert_eq!(DataType::parse_coe_name("BIT3"), DataType::BitN(3));
}

#[test]
fn unknown_typename_is_preserved_verbatim() {
    assert_eq!(
        DataType::parse_coe_name("DT1018"),
        DataType::Named("DT1018".to_owned())
    );
}
