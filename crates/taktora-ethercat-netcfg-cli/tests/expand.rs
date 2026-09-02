//! `TEST_0841` — `netcfg expand` prints the build-equivalent generated module
//! (`REQ_0832`): `run_expand` reads a network.yaml and returns exactly the
//! generated Rust module (byte-identical to driving `parse` + `generate`
//! directly).

use std::fs;

const NETWORK_YAML: &str = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, sm_watchdog_enabled: true, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }], rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels:
  - { name: "ethercat.wago.750-430.inputs",  device: coupler, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }
  - { name: "ethercat.wago.750-530.outputs", device: coupler, direction: rx, bit_offset: 0, bit_length: 8, element_type: u8 }
"#;

#[test]
fn run_expand_prints_the_generated_module() {
    let src_dir = tempfile::tempdir().expect("create yaml dir");
    let yaml_path = src_dir.path().join("network.yaml");
    fs::write(&yaml_path, NETWORK_YAML).expect("write network.yaml");

    let source = taktora_ethercat_netcfg_cli::run_expand(&yaml_path).expect("run_expand succeeds");

    assert!(
        syn::parse_file(&source).is_ok(),
        "expanded source parses as a Rust file"
    );
    assert!(
        source.contains("PDO_MAP"),
        "expanded source contains PDO_MAP"
    );

    let config = taktora_ethercat_netcfg::parse(NETWORK_YAML).expect("fixture parses");
    let expected = taktora_ethercat_netcfg_codegen::generate(&config).expect("fixture generates");
    assert_eq!(
        source, expected,
        "expand returns exactly the generated module"
    );
}
