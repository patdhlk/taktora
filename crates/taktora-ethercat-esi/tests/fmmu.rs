//! `REQ_0848` — `<Fmmu>` declarations captured in the IR, declaration order
//! preserved, unknown usages tolerated as `Other`.
use taktora_ethercat_esi::{FmmuUsage, parse};

fn esi(device_body: &str) -> String {
    format!(
        "<EtherCATInfo><Vendor><Id>#x2</Id></Vendor><Descriptions><Devices><Device>\
         <Type ProductCode=\"#x1\" RevisionNo=\"#x1\">T</Type>{device_body}\
         </Device></Devices></Descriptions></EtherCATInfo>"
    )
}

#[test]
fn fmmus_decode_known_usages_in_declaration_order() {
    let xml = esi("<Fmmu>Outputs</Fmmu><Fmmu>Inputs</Fmmu><Fmmu>MBoxState</Fmmu>");
    let file = parse(&xml).expect("parses");
    let fmmus = &file.devices[0].fmmus;
    assert_eq!(fmmus.len(), 3);
    assert_eq!(fmmus[0].usage, FmmuUsage::Outputs);
    assert_eq!(fmmus[1].usage, FmmuUsage::Inputs);
    assert_eq!(fmmus[2].usage, FmmuUsage::MBoxState);
}

#[test]
fn unknown_fmmu_usage_is_tolerated_as_other() {
    let xml = esi("<Fmmu>VendorMagic</Fmmu>");
    let file = parse(&xml).expect("unknown usage must not fail the parse");
    assert_eq!(
        file.devices[0].fmmus[0].usage,
        FmmuUsage::Other("VendorMagic".to_owned())
    );
}

#[test]
fn device_without_fmmus_has_empty_vec() {
    let file = parse(&esi("")).expect("parses");
    assert!(file.devices[0].fmmus.is_empty());
}

#[test]
fn fmmu_with_attribute_still_decodes_usage_text() {
    // Real Beckhoff files carry attributes on <Fmmu> (e.g. OpOnly="1" in
    // EL2004). The attribute must not break deserialization — only the usage
    // text matters.
    let xml = esi("<Fmmu OpOnly=\"1\">Outputs</Fmmu>");
    let file = parse(&xml).expect("attribute-bearing <Fmmu> must parse");
    assert_eq!(
        file.devices[0].fmmus[0].usage,
        FmmuUsage::Outputs,
        "<Fmmu OpOnly=\"1\"> should still decode to Outputs"
    );
}

#[test]
fn beckhoff_fixture_fmmus_are_captured() {
    let file = parse(include_str!("fixtures/beckhoff_el3602.xml")).expect("fixture parses");
    let fmmus = &file.devices[0].fmmus;
    assert_eq!(fmmus.len(), 2);
    assert!(
        fmmus.iter().any(|f| f.usage == FmmuUsage::Inputs),
        "EL3602 declares an Inputs FMMU"
    );
    assert!(
        fmmus.iter().any(|f| f.usage == FmmuUsage::MBoxState),
        "EL3602 declares an MBoxState FMMU"
    );
}
