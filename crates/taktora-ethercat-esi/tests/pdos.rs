//! `REQ_0504` — structured PDOs with full per-entry metadata, padding entries
//! preserved, assignment alternatives captured not resolved.
use taktora_ethercat_esi::{DataType, Pdo, PdoEntry, parse};

const EL3001: &str = include_str!("fixtures/el3001_like.xml");
const ALTERNATIVES: &str = include_str!("fixtures/pdo_alternatives.xml");

#[test]
fn txpdo_preserves_metadata_entries_and_padding() {
    let file = parse(EL3001).expect("fixture parses");
    let dev = &file.devices[0];
    assert_eq!(dev.tx_pdos.len(), 1);
    assert!(dev.rx_pdos.is_empty());

    let pdo = &dev.tx_pdos[0];
    assert_eq!(pdo.index, 0x1A00);
    assert_eq!(pdo.name.as_deref(), Some("AI TxPDO-Map"));
    assert_eq!(pdo.sm, Some(3));
    assert!(pdo.fixed);
    assert!(pdo.mandatory);

    assert_eq!(
        pdo.entries,
        vec![
            PdoEntry {
                index: 0x6000,
                sub_index: 1,
                bit_length: 1,
                name: Some("Underrange".into()),
                data_type: Some(DataType::Bool),
            },
            PdoEntry {
                index: 0x0000,
                sub_index: 0,
                bit_length: 7,
                name: None,
                data_type: None,
            },
            PdoEntry {
                index: 0x6000,
                sub_index: 17,
                bit_length: 16,
                name: Some("Value".into()),
                data_type: Some(DataType::I16),
            },
        ]
    );
}

#[test]
fn assignment_alternatives_are_captured_not_concatenated() {
    let file = parse(ALTERNATIVES).expect("fixture parses");
    let dev = &file.devices[0];
    assert_eq!(dev.tx_pdos.len(), 2);
    let indices: Vec<u16> = dev.tx_pdos.iter().map(|p: &Pdo| p.index).collect();
    assert_eq!(indices, vec![0x1A00, 0x1A01]);
}
