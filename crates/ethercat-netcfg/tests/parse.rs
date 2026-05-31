//! Integration test for the public `parse` entry point (`TEST_0830`).

use core::time::Duration;

use ethercat_netcfg::{
    ChannelBinding, DeviceInstance, DeviceSource, ElementType, NetworkConfig, PdoDirection,
    PdoEntry, parse,
};

/// `TEST_0830` — a minimal inline `network.yaml` parses into the IR.
#[test]
fn parses_minimal_inline_network() {
    let yaml = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
devices:
  - label: coupler
    pdos:
      tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }]
channels:
  - name: ethercat.wago.750-430.inputs
    device: coupler
    direction: tx
    bit_offset: 0
    bit_length: 8
    element_type: u8
  - name: ethercat.wago.750-530.outputs
    device: coupler
    direction: rx
    bit_offset: 0
    bit_length: 8
    element_type: u8
";

    let config: NetworkConfig = parse(yaml).expect("minimal inline config parses");

    assert_eq!(config.schema_version, 1);
    assert_eq!(config.bus.cycle_time, Duration::from_millis(2));
    assert!(!config.bus.distributed_clocks);
    assert_eq!(config.bus.max_subdevices, 16);
    assert_eq!(config.bus.max_pdi_bytes, 256);
    assert_eq!(config.bus.default_nic, None);

    assert_eq!(config.devices.len(), 1);
    let device: &DeviceInstance = &config.devices[0];
    assert_eq!(device.label, "coupler");
    assert_eq!(device.identity, None);
    assert_eq!(device.station_alias, None);
    assert_eq!(device.address_override, None);

    let DeviceSource::Inline { rx, tx } = &device.source else {
        panic!("inline device should resolve to DeviceSource::Inline");
    };
    assert!(rx.is_empty());
    assert_eq!(
        tx,
        &vec![PdoEntry {
            index: 0x6000,
            bit_offset: 0,
            bit_length: 8,
        }]
    );

    assert_eq!(config.channels.len(), 2);

    let inputs: &ChannelBinding = &config.channels[0];
    assert_eq!(inputs.name, "ethercat.wago.750-430.inputs");
    assert_eq!(inputs.device, "coupler");
    assert_eq!(inputs.direction, PdoDirection::Tx);
    assert_eq!(inputs.bit_offset, 0);
    assert_eq!(inputs.bit_length, 8);
    assert_eq!(inputs.element_type, ElementType::U8);
    assert!(!inputs.allow_overlap);

    let outputs: &ChannelBinding = &config.channels[1];
    assert_eq!(outputs.name, "ethercat.wago.750-530.outputs");
    assert_eq!(outputs.device, "coupler");
    assert_eq!(outputs.direction, PdoDirection::Rx);
    assert_eq!(outputs.bit_offset, 0);
    assert_eq!(outputs.bit_length, 8);
    assert_eq!(outputs.element_type, ElementType::U8);
    assert!(!outputs.allow_overlap);
}
