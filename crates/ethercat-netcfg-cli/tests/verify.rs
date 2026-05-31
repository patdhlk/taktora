//! Slice 21 / `TEST_0845-pin` (`REQ_0835`): `netcfg verify` detects pin
//! drift — a referenced ESI whose content hash or device revision no
//! longer matches the lockfile pin is an error.

use std::fs;

use ethercat_netcfg_cli::{CliError, run_fetch, run_verify};

const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000005">WAGO 750-354</Type>
    <Name>WAGO 750-354</Name>
    <TxPdo>
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>8</BitLen></Entry>
    </TxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

// Same fixture with the TxPdo entry BitLen changed 8 -> 16: the file
// content differs, so its SHA-256 no longer matches the pin.
const ESI_XML_TAMPERED: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000005">WAGO 750-354</Type>
    <Name>WAGO 750-354</Name>
    <TxPdo>
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>16</BitLen></Entry>
    </TxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

#[test]
fn verify_detects_pin_drift() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let root = tmp.path();

    // ESI fixture under `src/wago.xml`.
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    let esi_path = src_dir.join("wago.xml");
    fs::write(&esi_path, ESI_XML).expect("write ESI fixture");

    // network.yaml referencing it by absolute path, empty channels.
    let yaml_path = root.join("network.yaml");
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
    fs::write(&yaml_path, yaml).expect("write network.yaml");

    // Fetch to produce the lockfile + vendored copy.
    let vendor_dir = root.join("vendor/esi");
    let lockfile_path = root.join("network.lock");
    run_fetch(&yaml_path, &vendor_dir, &lockfile_path).expect("fetch should succeed");

    // Match case: nothing changed since fetch -> Ok.
    run_verify(&yaml_path, &lockfile_path).expect("verify should pass when nothing changed");

    // Mismatch case: tamper the ESI source (BitLen 8 -> 16) so its
    // content differs from the pin. The content hash check fires first.
    fs::write(&esi_path, ESI_XML_TAMPERED).expect("rewrite tampered ESI");
    let err = run_verify(&yaml_path, &lockfile_path).expect_err("verify should fail on pin drift");
    assert!(
        matches!(err, CliError::HashMismatch { .. }),
        "expected HashMismatch, got {err:?}",
    );
}
