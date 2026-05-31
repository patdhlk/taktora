//! `TEST_0832` (`REQ_0823`) — a channel's emitted `EthercatRouting` address is
//! resolved by the bound device's **label**, not by a frozen list position.
//!
//! Reordering the device list must re-resolve the channel's address to the
//! device's new bus-order index. This is a regression guard against a
//! positional binding (where the channel would freeze the index it saw at
//! emit time, or use its own channel index / a fixed value).

/// Devices in order `[coupler, din, dout]`: `din` at index 1 → `0x1001`.
const CONFIG_A: &str = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: din,     pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: dout,    pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels:
  - { name: "ethercat.din.inputs", device: din, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }
"#;

/// Same devices + same channel, list reordered `[dout, coupler, din]`:
/// `din` now at index 2 → `0x1002`.
const CONFIG_B: &str = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: dout,    pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
  - { label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: din,     pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
channels:
  - { name: "ethercat.din.inputs", device: din, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }
"#;

/// Find a `pub const <ident>` item and return its initializer expression.
fn const_init<'a>(file: &'a syn::File, ident: &str) -> &'a syn::Expr {
    file.items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(c) if c.ident == ident => Some(&*c.expr),
            _ => None,
        })
        .unwrap_or_else(|| panic!("generated source defines `pub const {ident}`"))
}

/// Pull the first `EthercatRouting::new(..)` argument (the resolved address)
/// out of a routing const initializer, as a `u16`.
fn routing_address(file: &syn::File, ident: &str) -> u16 {
    let init = const_init(file, ident);
    let syn::Expr::Call(call) = init else {
        panic!("routing const `{ident}` initializer is not a call expression");
    };
    let first = call
        .args
        .first()
        .unwrap_or_else(|| panic!("routing const `{ident}` has no arguments"));
    match first {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(lit),
            ..
        }) => lit
            .base10_parse::<u16>()
            .expect("address literal parses as u16"),
        other => {
            use quote::ToTokens;
            panic!(
                "first routing arg is not an int literal: {}",
                other.to_token_stream()
            )
        }
    }
}

/// Generate source for `yaml` and return the din channel's routing address.
fn din_address(yaml: &str) -> u16 {
    let cfg = ethercat_netcfg::parse(yaml).expect("config parses");
    let src = ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");
    let file = syn::parse_file(&src).expect("generated source is valid Rust");
    routing_address(&file, "ETHERCAT_DIN_INPUTS")
}

#[test]
fn channel_address_follows_device_label_not_list_position() {
    // Config A: din at index 1 → 0x1001.
    assert_eq!(
        din_address(CONFIG_A),
        0x1001,
        "din channel should resolve to din's address at index 1"
    );

    // Config B: same channel, din reordered to index 2 → 0x1002.
    // The binding tracked the label, so it re-resolved to the new position.
    assert_eq!(
        din_address(CONFIG_B),
        0x1002,
        "din channel should re-resolve to din's NEW address at index 2"
    );
}
