//! `REQ_0849` — `<Eeprom>` SII source data captured: hex payloads decoded to
//! bytes, no SII interpretation, unknown children kept as `RawXml`.
use taktora_ethercat_esi::{EsiError, parse};

fn esi(device_body: &str) -> String {
    format!(
        "<EtherCATInfo><Vendor><Id>#x2</Id></Vendor><Descriptions><Devices><Device>\
         <Type ProductCode=\"#x1\" RevisionNo=\"#x1\">T</Type>{device_body}\
         </Device></Devices></Descriptions></EtherCATInfo>"
    )
}

#[test]
fn eeprom_fields_decode() {
    let xml = esi(
        "<Eeprom><ByteSize>2048</ByteSize><ConfigData>0401000000000000</ConfigData>\
         <BootStrap>0010800080108000</BootStrap></Eeprom>",
    );
    let file = parse(&xml).expect("parses");
    let eeprom = file.devices[0].eeprom.as_ref().expect("eeprom present");
    assert_eq!(eeprom.byte_size, Some(2048));
    assert_eq!(eeprom.config_data, vec![0x04, 0x01, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        eeprom.bootstrap.as_deref(),
        Some(&[0x00, 0x10, 0x80, 0x00, 0x80, 0x10, 0x80, 0x00][..])
    );
    assert!(eeprom.categories.is_empty());
}

#[test]
fn device_without_eeprom_is_none() {
    let file = parse(&esi("")).expect("parses");
    assert!(file.devices[0].eeprom.is_none());
}

#[test]
fn bad_config_data_hex_is_a_located_value_error() {
    let xml = esi("<Eeprom><ConfigData>04xyz</ConfigData></Eeprom>");
    let err = parse(&xml).expect_err("bad hex must fail");
    match err {
        EsiError::Value { path, .. } => assert_eq!(path, "Eeprom.ConfigData"),
        other => panic!("expected Value error, got {other:?}"),
    }
}

#[test]
fn unknown_eeprom_children_are_captured_as_raw_xml() {
    let xml = esi("<Eeprom><ConfigData>0401</ConfigData>\
         <Category><CatNo>30</CatNo><Data>aabb</Data></Category></Eeprom>");
    let file = parse(&xml).expect("parses");
    let eeprom = file.devices[0].eeprom.as_ref().expect("eeprom present");
    assert_eq!(eeprom.categories.len(), 1);
    let cat = &eeprom.categories[0];
    assert_eq!(cat.name, "Category");
    assert_eq!(cat.children.len(), 2);
    assert_eq!(cat.children[0].name, "CatNo");
    assert_eq!(cat.children[0].text.as_deref(), Some("30"));
}

#[test]
fn eeprom_is_no_longer_a_vendor_extension() {
    let xml = esi("<Eeprom><ConfigData>0401</ConfigData></Eeprom>");
    let file = parse(&xml).expect("parses");
    assert!(
        file.devices[0]
            .vendor_extensions
            .iter()
            .all(|e| e.name != "Eeprom"),
        "typed Eeprom must not also appear as a vendor extension"
    );
}

#[test]
fn self_closing_eeprom_child_is_captured() {
    let xml = esi("<Eeprom><ConfigData>0401</ConfigData><Category/></Eeprom>");
    let file = parse(&xml).expect("parses");
    let eeprom = file.devices[0].eeprom.as_ref().expect("eeprom present");
    assert_eq!(eeprom.categories.len(), 1);
    assert_eq!(eeprom.categories[0].name, "Category");
    assert!(eeprom.categories[0].children.is_empty());
}

#[test]
fn beckhoff_fixture_eeprom_is_captured() {
    let file = parse(include_str!("fixtures/beckhoff_el1008.xml")).expect("fixture parses");
    let eeprom = file.devices[0]
        .eeprom
        .as_ref()
        .expect("EL1008 declares an Eeprom");
    assert_eq!(eeprom.byte_size, Some(2048));
    assert_eq!(eeprom.config_data, vec![0x04, 0x01, 0, 0, 0, 0, 0, 0]);
}
