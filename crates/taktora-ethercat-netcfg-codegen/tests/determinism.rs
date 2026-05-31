//! `TEST_0838` — determinism guard (`REQ_0829`).
//!
//! The same `network.yaml` must produce byte-identical generated source
//! across runs and machines, so the output is reviewable in diffs and
//! cacheable. No timestamp, no hash-map iteration order, and no source
//! key-order may leak into the output. This test pins that property with
//! two angles:
//!   1. Generating the same config many times yields one byte string.
//!   2. A semantically-identical config with mapping keys written in a
//!      different order yields the same byte string (source key-order
//!      does not leak).

/// Canonical non-trivial fixture: 3 devices (a mix of rx-only, tx-only,
/// and both) and 4 channels spread across them and both directions. Rich
/// enough that any iteration-order nondeterminism in the generator would
/// surface as differing output.
const CANONICAL_YAML: &str = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
devices:
  - label: coupler
    pdos:
      rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }]
      tx: [{ index: 0x6000, bit_offset: 0, bit_length: 16 }]
  - label: drive
    pdos:
      tx: [{ index: 0x6010, bit_offset: 0, bit_length: 32 }]
  - label: io_block
    pdos:
      rx: [{ index: 0x7010, bit_offset: 0, bit_length: 8 }]
channels:
  - name: ethercat.coupler.inputs
    device: coupler
    direction: tx
    bit_offset: 0
    bit_length: 16
    element_type: u16
  - name: ethercat.coupler.outputs
    device: coupler
    direction: rx
    bit_offset: 0
    bit_length: 8
    element_type: u8
  - name: ethercat.drive.position
    device: drive
    direction: tx
    bit_offset: 0
    bit_length: 32
    element_type: u32
  - name: ethercat.io_block.relays
    device: io_block
    direction: rx
    bit_offset: 0
    bit_length: 8
    element_type: u8
";

/// Semantically identical to [`CANONICAL_YAML`], but with mapping keys
/// written in a different order: `bus` keys are shuffled (`max_pdi_bytes`
/// before `cycle_time_ms`), and the keys inside each device and channel
/// mapping are reordered. Parsing is key-order-insensitive, so this must
/// produce byte-identical output.
const REORDERED_YAML: &str = r"
schema_version: 1
bus:
  max_pdi_bytes: 256
  max_subdevices: 16
  distributed_clocks: false
  cycle_time_ms: 2
devices:
  - pdos:
      tx: [{ bit_length: 16, index: 0x6000, bit_offset: 0 }]
      rx: [{ bit_offset: 0, bit_length: 8, index: 0x7000 }]
    label: coupler
  - label: drive
    pdos:
      tx: [{ bit_offset: 0, bit_length: 32, index: 0x6010 }]
  - pdos:
      rx: [{ index: 0x7010, bit_length: 8, bit_offset: 0 }]
    label: io_block
channels:
  - device: coupler
    direction: tx
    name: ethercat.coupler.inputs
    element_type: u16
    bit_length: 16
    bit_offset: 0
  - bit_offset: 0
    bit_length: 8
    element_type: u8
    direction: rx
    device: coupler
    name: ethercat.coupler.outputs
  - name: ethercat.drive.position
    element_type: u32
    bit_offset: 0
    bit_length: 32
    direction: tx
    device: drive
  - direction: rx
    name: ethercat.io_block.relays
    device: io_block
    bit_length: 8
    bit_offset: 0
    element_type: u8
";

#[test]
fn generated_source_is_byte_deterministic() {
    let cfg = taktora_ethercat_netcfg::parse(CANONICAL_YAML).expect("canonical config parses");

    // Sanity: the generated source is valid Rust.
    let first = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");
    syn::parse_file(&first).expect("generated source is valid Rust");

    // 1. Many runs over the same config are byte-identical. With any
    //    hash-map iteration order leaking into output, the random
    //    `RandomState` seed would make these diverge across iterations.
    for iteration in 0..50 {
        let again = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");
        assert_eq!(
            first, again,
            "generated source differs on iteration {iteration}; output is not byte-deterministic"
        );
    }

    // 2. Source key-order does not leak: a semantically identical config
    //    with reordered mapping keys produces the same bytes.
    let reordered_cfg =
        taktora_ethercat_netcfg::parse(REORDERED_YAML).expect("reordered config parses");
    assert_eq!(
        cfg, reordered_cfg,
        "fixtures must be semantically identical; only key order may differ"
    );
    let reordered_out =
        taktora_ethercat_netcfg_codegen::generate(&reordered_cfg).expect("codegen succeeds");
    assert_eq!(
        first, reordered_out,
        "key-reordered config produced different output; source key-order leaked into codegen"
    );
}
