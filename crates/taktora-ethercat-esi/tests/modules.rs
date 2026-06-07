//! `REQ_0850` — MDP `<Modules>` catalog and `<Slots>` constraints captured
//! faithfully; the parser never resolves a plugged configuration.
use taktora_ethercat_esi::{EsiError, parse};

const MODULAR: &str = include_str!("fixtures/modular_coupler.xml");

#[test]
fn module_catalog_is_captured() {
    let file = parse(MODULAR).expect("fixture parses");
    assert_eq!(file.modules.len(), 3);

    let di = &file.modules[0];
    assert_eq!(di.ident, Some(0x044D_2F52));
    assert_eq!(di.product_type.as_deref(), Some("DI-8"));
    assert_eq!(di.name.as_deref(), Some("8-channel digital input"));
    assert_eq!(di.tx_pdos.len(), 1);
    assert!(di.rx_pdos.is_empty());
    assert_eq!(di.tx_pdos[0].index, 0x1A00);
    assert_eq!(di.tx_pdos[0].entries[0].bit_length, 8);

    let dout = &file.modules[1];
    assert_eq!(dout.rx_pdos.len(), 1);
    assert_eq!(dout.rx_pdos[0].index, 0x1600);

    let end = &file.modules[2];
    assert_eq!(end.ident, Some(0x06EA_2F52));
    assert!(end.tx_pdos.is_empty() && end.rx_pdos.is_empty());
}

#[test]
fn slot_constraints_are_captured() {
    let file = parse(MODULAR).expect("fixture parses");
    let slots = file.devices[0].slots.as_ref().expect("device has slots");
    assert_eq!(slots.slot_pdo_increment, Some(16));
    assert_eq!(slots.slot_index_increment, Some(0x10));
    assert_eq!(slots.slots.len(), 2);

    let kbus = &slots.slots[0];
    assert_eq!(kbus.name.as_deref(), Some("KBus"));
    assert_eq!(kbus.min_instances, Some(0));
    assert_eq!(kbus.max_instances, Some(64));
    assert_eq!(kbus.module_idents.len(), 2);
    assert_eq!(kbus.module_idents[0].ident, 0x044D_2F52);
    assert!(kbus.module_idents[0].default);
    assert!(!kbus.module_idents[1].default);

    let end = &slots.slots[1];
    assert_eq!(end.min_instances, Some(1));
    assert_eq!(end.max_instances, Some(1));
    assert_eq!(end.name.as_deref(), Some("End"));
}

#[test]
fn non_modular_device_has_no_slots_and_file_has_no_modules() {
    let file = parse(include_str!("fixtures/beckhoff_el1008.xml")).expect("parses");
    assert!(file.devices[0].slots.is_none());
    assert!(file.modules.is_empty());
}

#[test]
fn slots_are_not_captured_as_vendor_extensions() {
    let file = parse(MODULAR).expect("fixture parses");
    assert!(
        file.devices[0]
            .vendor_extensions
            .iter()
            .all(|e| e.name != "Slots"),
        "typed Slots must not also appear as a vendor extension"
    );
}

#[test]
fn modules_only_file_parses_with_empty_devices() {
    let xml = "<EtherCATInfo><Vendor><Id>#x21</Id></Vendor><Descriptions><Modules>\
               <Module><Type ModuleIdent=\"#x1\">M-1</Type></Module>\
               </Modules></Descriptions></EtherCATInfo>";
    let file = parse(xml).expect("a modules-only ESI file is valid input");
    assert!(file.devices.is_empty());
    assert_eq!(file.modules.len(), 1);
    assert_eq!(file.modules[0].ident, Some(1));
}

#[test]
fn bad_slot_module_ident_is_a_number_error() {
    let xml = r##"<?xml version="1.0"?>
<EtherCATInfo><Vendor><Id>#x21</Id></Vendor><Descriptions><Devices>
<Device><Type ProductCode="#x1" RevisionNo="#x1">C</Type>
  <Slots><Slot><ModuleIdent>GARBAGE</ModuleIdent></Slot></Slots>
</Device>
</Devices></Descriptions></EtherCATInfo>"##;
    let err = parse(xml).expect_err("non-hex ModuleIdent in Slot must error");
    match err {
        EsiError::Number { path, raw } => {
            assert_eq!(raw, "GARBAGE");
            assert_eq!(path, "Slot.ModuleIdent", "path names the field: {path}");
        }
        other => panic!("expected Number error, got {other:?}"),
    }
}

#[test]
fn bad_module_type_ident_is_a_number_error() {
    let xml = r#"<?xml version="1.0"?>
<EtherCATInfo><Vendor><Id>#x21</Id></Vendor><Descriptions><Modules>
<Module><Type ModuleIdent="NaN">M</Type></Module>
</Modules></Descriptions></EtherCATInfo>"#;
    let err = parse(xml).expect_err("non-hex ModuleIdent on Module.Type must error");
    match err {
        EsiError::Number { path, raw } => {
            assert_eq!(raw, "NaN");
            assert_eq!(
                path, "Module.Type.ModuleIdent",
                "path names the field: {path}"
            );
        }
        other => panic!("expected Number error, got {other:?}"),
    }
}
