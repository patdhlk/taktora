//! `TEST_05xx` — parse trimmed single-device excerpts of real Beckhoff ESI
//! files. These exercise the messy-but-valid shapes real vendor files carry:
//! repeated localized `<Name LcId=…>`, CDATA-wrapped names, and a
//! `<DataType DScale=…>` attribute on entry data-type elements.
//!
//! The fixtures are trimmed UTF-8 excerpts (one `<Device>` each, transcoded
//! from ISO-8859-1, CRLF normalized); see the provenance comment in each file.

use taktora_ethercat_esi::{DataType, Identity, parse};

const EL2004: &str = include_str!("fixtures/beckhoff_el2004.xml");
const EL1008: &str = include_str!("fixtures/beckhoff_el1008.xml");
const EL3602: &str = include_str!("fixtures/beckhoff_el3602.xml");

#[test]
fn parses_el2004_multi_name_cdata_rxpdos() {
    let file = parse(EL2004).expect("EL2004 fixture parses");
    assert_eq!(file.vendor.id, 0x0000_0002, "Beckhoff vendor id");
    assert_eq!(file.devices.len(), 1);

    let dev = &file.devices[0];
    // English (LcId 1033) name wins over the German (1031) one, decoded from CDATA.
    assert_eq!(
        dev.name.as_deref(),
        Some("EL2004 4Ch. Dig. Output 24V, 0.5A"),
        "English CDATA name decodes"
    );
    assert_eq!(
        dev.identity,
        Identity {
            vendor_id: 0x0000_0002,
            product_code: 0x07d4_3052,
            revision: 0x0000_0000,
        }
    );

    // 4 RxPdos, each one BOOL Output entry.
    assert_eq!(dev.rx_pdos.len(), 4, "EL2004 has 4 RxPDOs");
    assert!(dev.tx_pdos.is_empty(), "EL2004 has no TxPDOs");
    for (i, pdo) in dev.rx_pdos.iter().enumerate() {
        assert_eq!(pdo.entries.len(), 1, "one entry per channel");
        let entry = &pdo.entries[0];
        assert_eq!(entry.name.as_deref(), Some("Output"));
        assert_eq!(entry.bit_length, 1);
        assert_eq!(entry.data_type, Some(DataType::parse_coe_name("BOOL")));
        assert_eq!(pdo.name.as_deref(), Some(&format!("Channel {}", i + 1)[..]));
    }
}

#[test]
fn parses_el1008_eight_input_channels() {
    let file = parse(EL1008).expect("EL1008 fixture parses");
    let dev = &file.devices[0];
    assert_eq!(
        dev.name.as_deref(),
        Some("EL1008 8Ch. Dig. Input 24V, 3ms"),
        "English CDATA name decodes"
    );
    assert_eq!(
        dev.identity,
        Identity {
            vendor_id: 0x0000_0002,
            product_code: 0x03f0_3052,
            revision: 0x0010_0000,
        }
    );
    // 8 input channels (TxPDOs), each one BOOL entry.
    assert_eq!(dev.tx_pdos.len(), 8, "EL1008 has 8 TxPDO channels");
    assert!(dev.rx_pdos.is_empty(), "EL1008 has no RxPDOs");
    for pdo in &dev.tx_pdos {
        assert_eq!(pdo.entries.len(), 1);
        assert_eq!(pdo.entries[0].bit_length, 1);
        assert_eq!(
            pdo.entries[0].data_type,
            Some(DataType::parse_coe_name("BOOL"))
        );
    }
}

#[test]
fn parses_el3602_two_txpdos_with_bit2_and_datatype_attr() {
    let file = parse(EL3602).expect("EL3602 fixture parses");
    let dev = &file.devices[0];
    assert_eq!(
        dev.name.as_deref(),
        Some("EL3602 2Ch. Ana. Input +/-10Volt, Diff. 24bit"),
        "English name (non-CDATA) decodes"
    );
    assert_eq!(
        dev.identity,
        Identity {
            vendor_id: 0x0000_0002,
            product_code: 0x0e12_3052,
            revision: 0x0010_0000,
        }
    );

    assert_eq!(dev.tx_pdos.len(), 2, "EL3602 has 2 TxPDOs");

    // A BIT2 entry is present with bit_length == 2.
    let bit2 = dev
        .tx_pdos
        .iter()
        .flat_map(|p| &p.entries)
        .find(|e| e.bit_length == 2)
        .expect("EL3602 has a BIT2 (2-bit) entry");
    assert_eq!(bit2.bit_length, 2);
    assert_eq!(bit2.data_type, Some(DataType::parse_coe_name("BIT2")));

    // The `<DataType DScale="+/-10">DINT</DataType>` attribute must not eat
    // the type text: the DINT "Value" entry still carries its data type.
    let dint = dev
        .tx_pdos
        .iter()
        .flat_map(|p| &p.entries)
        .find(|e| e.name.as_deref() == Some("Value"))
        .expect("EL3602 has a Value entry");
    assert_eq!(
        dint.data_type,
        Some(DataType::parse_coe_name("DINT")),
        "DataType text read despite DScale attribute"
    );
}
