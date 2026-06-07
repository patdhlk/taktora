//! `TEST_0835` — alongside `PDO_MAP`, `generate` emits, per `ChannelBinding`,
//! a named `pub const <NAME>: EthercatRouting` (constructed with the bound
//! device's resolved subdevice address, direction, bit offset, bit length)
//! plus a `pub const <NAME>_NAME: &str` carrying the original channel name.

/// Minimal WAGO bus: one coupler device, two channels (one tx, one rx).
const FIXTURE: &str = r#"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, sm_watchdog_enabled: true, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }], rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels:
  - { name: "ethercat.wago.750-430.inputs",  device: coupler, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }
  - { name: "ethercat.wago.750-530.outputs", device: coupler, direction: rx, bit_offset: 0, bit_length: 8, element_type: u8 }
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

/// Extract the call argument list of an `EthercatRouting::new(..)` const init.
fn routing_args(init: &syn::Expr) -> &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma> {
    match init {
        syn::Expr::Call(call) => &call.args,
        _ => panic!("routing const initializer is not a call expression"),
    }
}

/// Render an expression back to its token string (for substring assertions).
fn expr_str(expr: &syn::Expr) -> String {
    use quote::ToTokens;
    expr.to_token_stream().to_string()
}

/// Pull the string value out of a `pub const <NAME>_NAME: &str = "..."`.
fn name_const(file: &syn::File, ident: &str) -> String {
    match const_init(file, ident) {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => s.value(),
        other => panic!("`{ident}` is not a string literal: {}", expr_str(other)),
    }
}

#[test]
fn emits_named_routing_and_name_consts_per_channel() {
    let cfg = taktora_ethercat_netcfg::parse(FIXTURE).expect("fixture parses");
    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");

    let file = syn::parse_file(&src).expect("generated source is valid Rust");

    // --- tx channel: 750-430 inputs --------------------------------------
    let inputs = const_init(&file, "ETHERCAT_WAGO_750_430_INPUTS");
    let args = routing_args(inputs);
    let joined: Vec<String> = args.iter().map(expr_str).collect();
    let joined = joined.join(" | ");
    assert!(
        joined.contains("4096"),
        "address 0x1000 missing in {joined}"
    );
    assert!(
        joined.contains("PdoDirection :: Tx"),
        "Tx direction missing in {joined}"
    );
    assert!(
        joined.contains("0u32"),
        "bit_offset 0u32 missing in {joined}"
    );
    assert!(
        joined.contains("8u16"),
        "bit_length 8u16 missing in {joined}"
    );

    assert_eq!(
        name_const(&file, "ETHERCAT_WAGO_750_430_INPUTS_NAME"),
        "ethercat.wago.750-430.inputs"
    );

    // --- rx channel: 750-530 outputs -------------------------------------
    let outputs = const_init(&file, "ETHERCAT_WAGO_750_530_OUTPUTS");
    let joined: Vec<String> = routing_args(outputs).iter().map(expr_str).collect();
    let joined = joined.join(" | ");
    assert!(
        joined.contains("4096"),
        "address 0x1000 missing in {joined}"
    );
    assert!(
        joined.contains("PdoDirection :: Rx"),
        "Rx direction missing in {joined}"
    );
    assert!(
        joined.contains("0u32"),
        "bit_offset 0u32 missing in {joined}"
    );
    assert!(
        joined.contains("8u16"),
        "bit_length 8u16 missing in {joined}"
    );

    assert_eq!(
        name_const(&file, "ETHERCAT_WAGO_750_530_OUTPUTS_NAME"),
        "ethercat.wago.750-530.outputs"
    );
}
