//! `REQ_0505` — unrecognised device-level vendor extensions captured as `RawXml`.
use taktora_ethercat_esi::parse;

const EL3001: &str = include_str!("fixtures/el3001_like.xml");

#[test]
fn captures_vendor_specific_element_as_raw_xml() {
    let file = parse(EL3001).expect("fixture parses");
    let exts = &file.devices[0].vendor_extensions;
    // The <VendorSpecific> element under <Device> is not part of the known
    // schema -> captured verbatim.
    assert_eq!(exts.len(), 1);
    let ext = &exts[0];
    assert_eq!(ext.name, "VendorSpecific");
    // Its child is preserved recursively.
    assert_eq!(ext.children.len(), 1);
    let child = &ext.children[0];
    assert!(child.name.ends_with("Foo"));
    assert_eq!(child.text.as_deref(), Some("bar"));
    assert!(
        child
            .attributes
            .iter()
            .any(|(k, v)| k == "attr" && v == "1")
    );
}
