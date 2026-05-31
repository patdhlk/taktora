//! Integration test for the public `parse` entry point
//! (`REQ_0500` / `REQ_0504` — identity + `TxPDO` entries).

use ethercat_esi::{EsiDevice, EsiFile, Identity, PdoEntry, parse};

const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions>
    <Devices>
      <Device>
        <Type ProductCode="#x07500354" RevisionNo="#x00000001">WAGO 750-354</Type>
        <Name>WAGO 750-354</Name>
        <TxPdo>
          <Index>#x1a00</Index>
          <Name>DI Inputs</Name>
          <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen><Name>Input</Name></Entry>
        </TxPdo>
      </Device>
    </Devices>
  </Descriptions>
</EtherCATInfo>
"##;

#[test]
fn parses_identity_and_tx_pdos() {
    let file = parse(ESI_XML).expect("ESI fixture should parse");

    assert_eq!(
        file,
        EsiFile {
            devices: vec![EsiDevice {
                identity: Identity {
                    vendor_id: 0x21,
                    product_code: 0x0750_0354,
                    revision: 1,
                },
                tx_pdos: vec![PdoEntry {
                    index: 0x6000,
                    bit_offset: 0,
                    bit_length: 8,
                }],
                rx_pdos: vec![],
            }],
        }
    );
}
