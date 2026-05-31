//! Slice 20: local-only ESI resolution (`REQ_0834`, hermetic builds).
//!
//! ESI references must resolve from LOCAL files only. A `file://` URL is
//! stripped to a local path and resolved like a bare path; a remote
//! `http://` / `https://` URL is rejected without any network access.

use std::io::Write;

use taktora_ethercat_netcfg::{NetcfgError, parse};

const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000001">WAGO 750-354</Type>
    <Name>WAGO 750-354</Name>
    <TxPdo>
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>8</BitLen></Entry>
    </TxPdo>
    <RxPdo>
      <Index>#x1600</Index>
      <Entry><Index>#x7000</Index><BitLen>8</BitLen></Entry>
    </RxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

/// A remote `https://` ESI reference is rejected without touching the
/// network. There is no network code anywhere, so a rejection at parse
/// time also proves the build stays hermetic.
#[test]
fn https_esi_reference_is_rejected() {
    let yaml = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: drive, esi: "https://example.com/device.xml" }
channels: []
"#;

    let result = parse(yaml);
    assert!(
        matches!(result, Err(NetcfgError::RemoteEsiNotVendored { .. })),
        "https reference should be rejected as remote, got {result:?}"
    );

    // The same rule applies to plain http://.
    let yaml_http = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: drive, esi: "http://example.com/device.xml" }
channels: []
"#;

    let result_http = parse(yaml_http);
    assert!(
        matches!(result_http, Err(NetcfgError::RemoteEsiNotVendored { .. })),
        "http reference should be rejected as remote, got {result_http:?}"
    );
}

/// A `file://` URL resolves exactly like the bare absolute path it wraps:
/// the scheme is stripped to a local path and read from the filesystem. The
/// resulting device is byte-for-byte identical to one that references the
/// bare path.
#[test]
fn file_url_resolves_like_local_path() {
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(ESI_XML.as_bytes())
        .expect("write ESI fixture");
    let esi_path = esi.path().to_path_buf();

    // The reference is `file://` + the absolute path, i.e. `file:///...`.
    let abs = esi_path.display();
    let yaml_file_url = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - {{ label: coupler, esi: "file://{abs}" }}
channels: []
"#,
    );
    let yaml_bare = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - {{ label: coupler, esi: "{abs}" }}
channels: []
"#,
    );

    let via_url = parse(&yaml_file_url).expect("file:// reference should parse");
    let via_bare = parse(&yaml_bare).expect("bare path reference should parse");

    // file:// resolves to the same path, so the resolved device — including
    // its DeviceSource::Esi PDOs and identity — is identical.
    assert_eq!(via_url.devices, via_bare.devices);
}
