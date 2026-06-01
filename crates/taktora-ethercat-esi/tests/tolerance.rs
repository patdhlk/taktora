//! `REQ_0504` tolerance branches: defaulting absent attributes to 0 and the
//! `pick_name` "first non-empty when no English" fallback. Each test builds a
//! minimal valid ESI wrapper inline (no fixture file) and would FAIL if the
//! defaults were reverted to hard errors.
//!
//! Integer attributes use plain decimal here (rather than the `#x` hex form) so
//! the XML never produces a `"#` sequence that would force `r##"…"##` raw-string
//! delimiters; `parse_esi_uint` accepts both forms and is exercised elsewhere.
use taktora_ethercat_esi::parse;

/// A `<Type>` with no `ProductCode`/`RevisionNo` attributes (a placeholder /
/// abstract module slot) must parse with both identity fields defaulting to 0
/// rather than rejecting the whole document.
#[test]
fn type_without_product_code_or_revision_defaults_identity_to_zero() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>2</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    <Device><Type>EL9999</Type></Device>
  </Devices></Descriptions>
</EtherCATInfo>"#;

    let file = parse(xml).expect("placeholder Type parses");
    let dev = &file.devices[0];
    assert_eq!(dev.identity.product_code, 0);
    assert_eq!(dev.identity.revision, 0);
    // Vendor id still propagates even with a placeholder Type.
    assert_eq!(dev.identity.vendor_id, 2);
}

/// A disabled `<Sm Enable="0">` with neither `StartAddress` nor `ControlByte`
/// is simply unconfigured: both must default to 0, not error.
#[test]
fn sm_without_start_address_or_control_byte_defaults_to_zero() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>2</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="256" RevisionNo="1">A</Type>
      <Sm Enable="0">Outputs</Sm>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"#;

    let file = parse(xml).expect("unconfigured Sm parses");
    let sms = &file.devices[0].sync_managers;
    assert_eq!(sms.len(), 1);
    assert_eq!(sms[0].start_address, 0);
    assert_eq!(sms[0].control_byte, 0);
    assert!(!sms[0].enable);
}

/// A device whose only `<Name>` carries a non-English `LcId` (1031, German)
/// falls back to that first non-empty name.
#[test]
fn name_falls_back_to_first_non_english_when_no_english() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>2</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="256" RevisionNo="1">A</Type>
      <Name LcId="1031">German Only</Name>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"#;

    let file = parse(xml).expect("German-only name parses");
    assert_eq!(file.devices[0].name.as_deref(), Some("German Only"));
}

/// With both English (1033) and German (1031) names present, `pick_name`
/// prefers the English entry regardless of document order.
#[test]
fn name_prefers_english_over_german() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>2</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="256" RevisionNo="1">A</Type>
      <Name LcId="1031">Deutsch</Name>
      <Name LcId="1033">English</Name>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"#;

    let file = parse(xml).expect("bilingual name parses");
    assert_eq!(file.devices[0].name.as_deref(), Some("English"));
}
