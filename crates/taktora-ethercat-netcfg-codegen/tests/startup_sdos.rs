//! Generated `PDO_MAP` chains `.with_startup_sdos` for devices declaring them.
use taktora_ethercat_netcfg::parse;
use taktora_ethercat_netcfg_codegen::generate;

#[test]
fn emits_with_startup_sdos() {
    let yaml = "schema_version: 1\nbus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }\ndevices:\n  - label: stepper\n    pdos: { rx: [{ index: 0x1601, bit_offset: 0, bit_length: 176 }] }\n    sm_watchdog_enabled: true\n    startup_sdos:\n      - { index: 0x8010, subindex: 0x01, type: u16, value: 1800 }\n      - { index: 0x8011, subindex: 0x02, type: i32, value: -5 }\nchannels: []\n";
    let cfg = parse(yaml).expect("parses");
    let src = generate(&cfg).expect("generates");
    assert!(src.contains(".with_startup_sdos"), "src:\n{src}");
    // prettyplease spacing is unstable: accept spaced or unspaced forms.
    assert!(
        src.contains("U16 (1800") || src.contains("U16(1800"),
        "src:\n{src}"
    );
    assert!(
        src.contains("I32 (- 5") || src.contains("I32(-5"),
        "src:\n{src}"
    );
    assert!(
        src.contains("taktora_connector_ethercat :: StartupSdo")
            || src.contains("taktora_connector_ethercat::StartupSdo"),
        "src:\n{src}"
    );
}

#[test]
fn omits_with_startup_sdos_when_none() {
    let yaml = "schema_version: 1\nbus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }\ndevices:\n  - label: din\n    pdos: { tx: [{ index: 0x1a00, bit_offset: 0, bit_length: 8 }] }\nchannels: []\n";
    let cfg = parse(yaml).expect("parses");
    let src = generate(&cfg).expect("generates");
    assert!(!src.contains(".with_startup_sdos"), "src:\n{src}");
}
