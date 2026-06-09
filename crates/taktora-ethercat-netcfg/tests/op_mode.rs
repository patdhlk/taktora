//! `op_mode` selects an `AlternativeSmMapping` on an `esi:` device (PDO-granularity).
use std::io::Write;
use taktora_ethercat_netcfg::{DeviceSource, NetcfgError, parse};

// Two named RxPDO mappings; "Positioning interface" is non-default.
const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000002</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x1b773052" RevisionNo="#x00170000">EL7047</Type>
    <Sm StartAddress="#x1000" ControlByte="#x44" Enable="1">Outputs</Sm>
    <Sm StartAddress="#x1400" ControlByte="#x00" Enable="1">Inputs</Sm>
    <RxPdo Sm="2"><Index>#x1600</Index><Entry><Index>#x7010</Index><BitLen>16</BitLen></Entry></RxPdo>
    <RxPdo><Index>#x1601</Index><Entry><Index>#x7010</Index><BitLen>48</BitLen></Entry></RxPdo>
    <TxPdo Sm="3"><Index>#x1a00</Index><Entry><Index>#x6010</Index><BitLen>16</BitLen></Entry></TxPdo>
    <TxPdo><Index>#x1a01</Index><Entry><Index>#x6010</Index><BitLen>48</BitLen></Entry></TxPdo>
    <Info><VendorSpecific><TwinCAT>
      <AlternativeSmMapping Default="1"><Name>Velocity control compact</Name>
        <Sm No="2"><Pdo>#x1600</Pdo></Sm><Sm No="3"><Pdo>#x1a00</Pdo></Sm>
      </AlternativeSmMapping>
      <AlternativeSmMapping><Name>Positioning interface</Name>
        <Sm No="2"><Pdo>#x1601</Pdo></Sm><Sm No="3"><Pdo>#x1a01</Pdo></Sm>
      </AlternativeSmMapping>
    </TwinCAT></VendorSpecific></Info>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

fn yaml_for(device_line: &str, esi_path: &str) -> String {
    format!(
        "schema_version: 1\nbus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}\ndevices:\n{}\nchannels: []\n",
        device_line.replace("{ESI}", esi_path)
    )
}

fn write_esi() -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(".xml").tempfile().unwrap();
    f.write_all(ESI_XML.as_bytes()).unwrap();
    f
}

#[test]
fn op_mode_selects_named_mapping() {
    let esi = write_esi();
    let yaml = yaml_for(
        "  - { label: stepper, esi: \"{ESI}\", op_mode: \"Positioning interface\", sm_watchdog_enabled: true }",
        &esi.path().display().to_string(),
    );
    let cfg = parse(&yaml).expect("parses");
    match &cfg.devices[0].source {
        DeviceSource::Esi { rx, tx, .. } => {
            assert_eq!(rx.iter().map(|e| e.index).collect::<Vec<_>>(), vec![0x1601]);
            assert_eq!(rx[0].bit_length, 48);
            assert_eq!(tx.iter().map(|e| e.index).collect::<Vec<_>>(), vec![0x1a01]);
        }
        other @ DeviceSource::Inline { .. } => panic!("expected Esi source, got {other:?}"),
    }
}

#[test]
fn omitted_op_mode_uses_default_mapping() {
    let esi = write_esi();
    let yaml = yaml_for(
        "  - { label: stepper, esi: \"{ESI}\", sm_watchdog_enabled: true }",
        &esi.path().display().to_string(),
    );
    let cfg = parse(&yaml).expect("parses");
    let DeviceSource::Esi { rx, .. } = &cfg.devices[0].source else {
        panic!()
    };
    assert_eq!(rx[0].index, 0x1600);
}

#[test]
fn unknown_op_mode_errors_with_available() {
    let esi = write_esi();
    let yaml = yaml_for(
        "  - { label: stepper, esi: \"{ESI}\", op_mode: \"nope\", sm_watchdog_enabled: true }",
        &esi.path().display().to_string(),
    );
    match parse(&yaml) {
        Err(NetcfgError::OpModeNotFound {
            requested,
            available,
            ..
        }) => {
            assert_eq!(requested, "nope");
            assert!(available.contains(&"Positioning interface".to_string()));
        }
        other => panic!("expected OpModeNotFound, got {other:?}"),
    }
}

#[test]
fn op_mode_without_esi_errors() {
    let yaml = yaml_for(
        "  - { label: x, op_mode: \"Positioning interface\", pdos: { rx: [{ index: 0x1600, bit_offset: 0, bit_length: 8 }] }, sm_watchdog_enabled: true }",
        "",
    );
    assert!(matches!(
        parse(&yaml),
        Err(NetcfgError::OpModeOnFlatDevice { .. })
    ));
}
