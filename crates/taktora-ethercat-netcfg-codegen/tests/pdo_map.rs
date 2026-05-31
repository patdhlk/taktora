//! `TEST_0834` — end-to-end tracer: parse the minimal inline fixture,
//! generate Rust source, and assert the output is a syntactically valid
//! `static PDO_MAP` carrying the device's inline PDO entry.

/// The same minimal inline fixture used by slice 1's parser test
/// (`TEST_0830`): one `coupler` device with a single `TxPDO` and two
/// channels.
const MINIMAL_YAML: &str = r"
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
      rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }]
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

#[test]
fn generates_valid_pdo_map_static() {
    let cfg = taktora_ethercat_netcfg::parse(MINIMAL_YAML).expect("minimal inline config parses");

    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");

    // Behavioral check #1: the output is syntactically valid Rust.
    let file = syn::parse_file(&src).expect("generated source is valid Rust");

    // Behavioral check #2: a top-level `static PDO_MAP` exists in the AST.
    let has_pdo_map = file.items.iter().any(|item| match item {
        syn::Item::Static(s) => s.ident == "PDO_MAP",
        _ => false,
    });
    assert!(
        has_pdo_map,
        "generated source defines a `static PDO_MAP`:\n{src}"
    );

    // Behavioral check #3: the device's inline TxPDO entry is carried
    // through. The index literal may be emitted as hex or decimal.
    assert!(
        src.contains("0x6000") || src.contains("24576"),
        "generated source carries the TxPDO index 0x6000:\n{src}"
    );
    assert!(
        src.contains("bit_length: 8") || src.contains("8u16"),
        "generated source carries the TxPDO bit_length 8:\n{src}"
    );
}
