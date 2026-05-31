//! Slice 15: `RxPDO` parsing (`REQ_0504`) + malformed-XML error surfacing
//! (`REQ_0506`-flavoured).

use taktora_ethercat_esi::{EsiError, PdoEntry, parse};

const FIXTURE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000001">WAGO</Type>
    <Name>WAGO</Name>
    <TxPdo>
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>8</BitLen></Entry>
      <Entry><Index>#x6001</Index><BitLen>8</BitLen></Entry>
    </TxPdo>
    <RxPdo>
      <Index>#x1600</Index>
      <Entry><Index>#x7000</Index><BitLen>8</BitLen></Entry>
    </RxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

#[test]
fn parses_rx_pdos_and_multi_entry_offsets() {
    let esi = parse(FIXTURE).expect("fixture should parse");

    assert_eq!(esi.devices.len(), 1, "exactly one device");
    let device = &esi.devices[0];

    assert_eq!(
        device.tx_pdos,
        [
            PdoEntry {
                index: 0x6000,
                bit_offset: 0,
                bit_length: 8,
            },
            PdoEntry {
                index: 0x6001,
                bit_offset: 8,
                bit_length: 8,
            },
        ],
    );

    assert_eq!(
        device.rx_pdos,
        [PdoEntry {
            index: 0x7000,
            bit_offset: 0,
            bit_length: 8,
        }],
    );
}

#[test]
fn malformed_xml_is_an_error() {
    let err = parse("<EtherCATInfo><Vendor>").expect_err("unclosed XML must error");
    assert!(matches!(err, EsiError::Xml(..)));
}
