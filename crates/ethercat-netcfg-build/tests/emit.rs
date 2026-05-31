//! `TEST_0830` / `REQ_0830` — `emit` generates a module into a caller `OUT_DIR`.

use std::fs;

const NETWORK_YAML: &str = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }], rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels:
  - { name: "ethercat.wago.750-430.inputs",  device: coupler, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }
  - { name: "ethercat.wago.750-530.outputs", device: coupler, direction: rx, bit_offset: 0, bit_length: 8, element_type: u8 }
"#;

#[test]
fn emit_generates_network_module_into_out_dir() {
    let out_dir = tempfile::tempdir().expect("create fake OUT_DIR");
    let src_dir = tempfile::tempdir().expect("create yaml dir");
    let yaml_path = src_dir.path().join("network.yaml");
    fs::write(&yaml_path, NETWORK_YAML).expect("write network.yaml");

    let outcome = ethercat_netcfg_build::emit(&yaml_path, out_dir.path()).expect("emit succeeds");

    let expected = out_dir.path().join("network.rs");
    assert_eq!(outcome.generated, expected);
    assert!(outcome.generated.exists(), "generated file exists on disk");

    let generated = fs::read_to_string(&outcome.generated).expect("read generated source");
    assert!(
        syn::parse_file(&generated).is_ok(),
        "generated source parses as a Rust file"
    );
    assert!(
        generated.contains("PDO_MAP"),
        "generated source contains PDO_MAP"
    );
}
