//! Slice 19 / `TEST_0842` (`REQ_0833`): `netcfg fetch` vendors the
//! referenced ESI files into a vendor directory and pins each by SHA-256
//! in a JSON lockfile.

use std::fmt::Write as _;
use std::fs;

use ethercat_netcfg_cli::{Lockfile, run_fetch};
use sha2::{Digest, Sha256};

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

#[test]
fn fetch_vendors_and_pins_esi() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let root = tmp.path();

    // ESI fixture under `src/wago.xml`.
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    let esi_path = src_dir.join("wago.xml");
    fs::write(&esi_path, ESI_XML).expect("write ESI fixture");

    // network.yaml referencing it by absolute path.
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

    let vendor_dir = root.join("vendor/esi");
    let lockfile_path = root.join("network.lock");

    let lockfile =
        run_fetch(&yaml_path, &vendor_dir, &lockfile_path).expect("fetch should succeed");

    // One ESI device -> one lock entry.
    assert_eq!(lockfile.entries.len(), 1);
    let entry = &lockfile.entries[0];

    // Revision pinned from RevisionNo #x00000005.
    assert_eq!(entry.revision, 5);

    // Vendored file exists on disk and matches the source bytes.
    assert!(entry.vendored.exists(), "vendored file should exist");
    let src_bytes = fs::read(&esi_path).expect("read source ESI");
    let vendored_bytes = fs::read(&entry.vendored).expect("read vendored ESI");
    assert_eq!(vendored_bytes, src_bytes);

    // SHA-256 matches an independent computation over the original bytes.
    let expected_hex = {
        let digest = Sha256::digest(&src_bytes);
        digest.iter().fold(String::new(), |mut acc, b| {
            write!(acc, "{b:02x}").expect("write to String");
            acc
        })
    };
    assert_eq!(entry.sha256, expected_hex);
    assert_eq!(entry.sha256.len(), 64);

    // Lockfile JSON exists on disk and round-trips.
    let lockfile_json = fs::read_to_string(&lockfile_path).expect("read lockfile");
    let parsed: Lockfile = serde_json::from_str(&lockfile_json).expect("lockfile JSON round-trips");
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].sha256, entry.sha256);
    assert_eq!(parsed.entries[0].revision, entry.revision);
    assert_eq!(parsed.entries[0].vendored, entry.vendored);
    assert_eq!(parsed.entries[0].reference, entry.reference);
}
