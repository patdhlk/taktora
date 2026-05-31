//! `REQ_0500` (pure parse) + `REQ_0504` (identity in IR).
use taktora_ethercat_esi::{Identity, parse};

const EL3001: &str = include_str!("fixtures/el3001_like.xml");

#[test]
fn parses_vendor_and_device_identity() {
    let file = parse(EL3001).expect("fixture parses");
    assert_eq!(file.vendor.id, 0x0000_0002);
    assert_eq!(file.vendor.name.as_deref(), Some("Synthetic Vendor"));
    assert_eq!(file.devices.len(), 1);

    let dev = &file.devices[0];
    assert_eq!(
        dev.identity,
        Identity {
            vendor_id: 0x0000_0002,
            product_code: 0x0bb9_3052,
            revision: 0x0010_0000
        }
    );
    assert_eq!(dev.name.as_deref(), Some("EL3001-like 1Ch. Ana. Input"));
    assert_eq!(dev.product_type.as_deref(), Some("EL3001-like"));
    assert_eq!(dev.group_type.as_deref(), Some("AnaIn"));
}
