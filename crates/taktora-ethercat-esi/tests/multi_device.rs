//! `REQ_0504` — all devices in a multi-device ESI file are parsed in order.
use taktora_ethercat_esi::parse;

const MULTI: &str = include_str!("fixtures/multi_device.xml");

#[test]
fn parses_all_devices_in_document_order() {
    let file = parse(MULTI).expect("fixture parses");
    let codes: Vec<u32> = file
        .devices
        .iter()
        .map(|d| d.identity.product_code)
        .collect();
    assert_eq!(codes, vec![0x0000_0100, 0x0000_0200]);
    // Vendor id propagates to every device's identity.
    assert!(
        file.devices
            .iter()
            .all(|d| d.identity.vendor_id == 0x0000_0002)
    );
}
