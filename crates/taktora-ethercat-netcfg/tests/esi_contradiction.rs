//! When a device declares BOTH `esi:` and inline `pdos:`, the parser
//! compares the ESI-resolved tx/rx entries against the inline entries. If
//! they disagree it is an [`NetcfgError::EsiContradiction`]; if they are
//! identical it is redundant-but-allowed (ESI is the source of truth).

use std::io::Write as _;

use taktora_ethercat_netcfg::{NetcfgError, parse};

/// A single-device ESI with one `TxPDO` entry: `#x6000`, `BitLen` 8.
const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000001">Coupler</Type>
    <Name>Coupler</Name>
    <TxPdo>
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>8</BitLen></Entry>
    </TxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

fn esi_yaml(inline_bit_length: u16) -> (tempfile::NamedTempFile, String) {
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(ESI_XML.as_bytes())
        .expect("write ESI fixture");
    let esi_path = esi.path().to_str().expect("ESI path is UTF-8").to_owned();

    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - label: coupler
    esi: "{esi_path}"
    pdos:
      tx:
        - {{ index: 0x6000, bit_offset: 0, bit_length: {inline_bit_length} }}
channels: []
"#
    );
    (esi, yaml)
}

#[test]
fn esi_and_contradicting_inline_pdos_is_an_error() {
    // Inline tx entry disagrees with the ESI (bit_length 16 vs ESI's 8).
    let (_esi, yaml) = esi_yaml(16);

    let err = parse(&yaml).expect_err("contradicting esi + inline pdos must error");
    assert!(
        matches!(err, NetcfgError::EsiContradiction { ref label } if label == "coupler"),
        "expected EsiContradiction {{ label: \"coupler\" }}, got {err:?}"
    );
}

#[test]
fn esi_and_matching_inline_pdos_is_allowed() {
    // Inline tx entry matches the ESI (bit_length 8): redundant but legal.
    let (_esi, yaml) = esi_yaml(8);

    parse(&yaml).expect("matching esi + inline pdos should parse");
}
