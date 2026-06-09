//! Per-device startup SDOs parse into the IR with per-type range validation.
use taktora_ethercat_netcfg::{NetcfgError, SdoValueSpec, parse};

fn yaml(sdo_lines: &str) -> String {
    format!(
        "schema_version: 1\nbus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}\ndevices:\n  - label: stepper\n    pdos: {{ rx: [{{ index: 0x1601, bit_offset: 0, bit_length: 176 }}] }}\n    sm_watchdog_enabled: true\n    startup_sdos:\n{sdo_lines}\nchannels: []\n"
    )
}

#[test]
fn parses_startup_sdos_in_order_with_types() {
    let cfg = parse(&yaml(
        "      - { index: 0x8010, subindex: 0x01, type: u16, value: 1800 }\n      - { index: 0x8011, subindex: 0x02, type: i32, value: -5 }",
    ))
    .expect("parses");
    let sdos = &cfg.devices[0].startup_sdos;
    assert_eq!(sdos.len(), 2);
    assert_eq!(sdos[0].index, 0x8010);
    assert_eq!(sdos[0].subindex, 0x01);
    assert_eq!(sdos[0].value, SdoValueSpec::U16(1800));
    assert_eq!(sdos[1].value, SdoValueSpec::I32(-5));
}

#[test]
fn out_of_range_value_for_type_errors() {
    // 70000 does not fit u16.
    let err = parse(&yaml(
        "      - { index: 0x8010, subindex: 0x01, type: u16, value: 70000 }",
    ))
    .unwrap_err();
    assert!(
        matches!(err, NetcfgError::SdoValueOutOfRange { .. }),
        "got {err:?}"
    );
}

#[test]
fn no_startup_sdos_is_empty() {
    let cfg = parse(
        "schema_version: 1\nbus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }\ndevices:\n  - label: din\n    pdos: { tx: [{ index: 0x1a00, bit_offset: 0, bit_length: 8 }] }\nchannels: []\n",
    )
    .expect("parses");
    assert!(cfg.devices[0].startup_sdos.is_empty());
}
