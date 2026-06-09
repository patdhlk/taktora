//! Slice 16: the parser resolves `esi:` device references at parse time
//! (`REQ_0824` — ESI resolution is the parser's only filesystem access).

use std::io::Write;

use taktora_ethercat_netcfg::{DeviceSource, Identity, PdoEntry, parse};

const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000001">WAGO 750-354</Type>
    <Name>WAGO 750-354</Name>
    <Sm StartAddress="#x1000" ControlByte="#x40" Enable="1">Outputs</Sm>
    <TxPdo Sm="3">
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>8</BitLen></Entry>
    </TxPdo>
    <RxPdo Sm="0">
      <Index>#x1600</Index>
      <Entry><Index>#x7000</Index><BitLen>8</BitLen></Entry>
    </RxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

#[test]
fn parser_resolves_esi_reference() {
    // Write the single-device ESI fixture to a temp file.
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(ESI_XML.as_bytes())
        .expect("write ESI fixture");
    let esi_path = esi.path().to_path_buf();

    // Reference that ESI file by absolute path from the network config.
    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - {{ label: coupler, esi: "{}" }}
channels: []
"#,
        esi_path.display(),
    );

    let config = parse(&yaml).expect("config with esi reference should parse");

    assert_eq!(config.devices.len(), 1);
    let device = &config.devices[0];
    assert_eq!(device.label, "coupler");

    assert_eq!(
        device.source,
        DeviceSource::Esi {
            path: esi_path,
            // PDO-granularity: index = PDO mapping-object index (0x1a00 / 0x1600),
            // bit_length = sum of the PDO's inner entries.
            tx: vec![PdoEntry {
                index: 0x1a00,
                bit_offset: 0,
                bit_length: 8
            }],
            rx: vec![PdoEntry {
                index: 0x1600,
                bit_offset: 0,
                bit_length: 8
            }],
        }
    );

    assert_eq!(
        device.identity,
        Some(Identity {
            vendor_id: 0x21,
            product_code: 0x0750_0354,
            revision: 1,
        })
    );
}
