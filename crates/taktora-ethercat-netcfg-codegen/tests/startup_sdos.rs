//! Generated `PDO_MAP` chains `.with_startup_sdos` for devices declaring them.
use taktora_ethercat_netcfg::parse;
use taktora_ethercat_netcfg_codegen::generate;

#[test]
fn emits_with_startup_sdos() {
    let yaml = "schema_version: 1\nbus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }\ndevices:\n  - label: stepper\n    pdos: { rx: [{ index: 0x1601, bit_offset: 0, bit_length: 176 }] }\n    sm_watchdog_enabled: true\n    startup_sdos:\n      - { index: 0x8010, subindex: 0x01, type: u16, value: 1800 }\n      - { index: 0x8011, subindex: 0x02, type: i32, value: -5 }\nchannels: []\n";
    let cfg = parse(yaml).expect("parses");
    let src = generate(&cfg).expect("generates");
    // generate() always runs prettyplease::unparse, so the output is formatted.
    assert!(src.contains(".with_startup_sdos"), "src:\n{src}");
    assert!(src.contains("SdoValue::U16(1800u16)"), "src:\n{src}");
    assert!(src.contains("SdoValue::I32(-5i32)"), "src:\n{src}");
    assert!(
        src.contains("taktora_connector_ethercat::StartupSdo"),
        "src:\n{src}"
    );
}

#[test]
fn emits_startup_sdos_without_watchdog() {
    // An input-only device (no rx PDOs -> no watchdog) that still needs PRE-OP
    // configuration: the chain is `SubDeviceMap::new(..).with_startup_sdos(..)`
    // with no `.with_sm_watchdog(..)`.
    let yaml = "schema_version: 1\nbus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }\ndevices:\n  - label: input_dev\n    pdos: { tx: [{ index: 0x1a00, bit_offset: 0, bit_length: 8 }] }\n    startup_sdos:\n      - { index: 0x8020, subindex: 0x01, type: u8, value: 1 }\nchannels: []\n";
    let cfg = parse(yaml).expect("parses");
    let src = generate(&cfg).expect("generates");
    assert!(src.contains(".with_startup_sdos"), "src:\n{src}");
    assert!(!src.contains(".with_sm_watchdog"), "src:\n{src}");
    assert!(src.contains("SdoValue::U8(1u8)"), "src:\n{src}");
}

#[test]
fn omits_with_startup_sdos_when_none() {
    let yaml = "schema_version: 1\nbus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }\ndevices:\n  - label: din\n    pdos: { tx: [{ index: 0x1a00, bit_offset: 0, bit_length: 8 }] }\nchannels: []\n";
    let cfg = parse(yaml).expect("parses");
    let src = generate(&cfg).expect("generates");
    assert!(!src.contains(".with_startup_sdos"), "src:\n{src}");
}
