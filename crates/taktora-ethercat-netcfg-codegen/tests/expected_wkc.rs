//! `TEST_0837` — each generated `SubDeviceMap.expected_wkc` is derived from
//! its inline PDO directions per the canonical `EtherCAT` `LRW`
//! working-counter rule: +1 per `SubDevice` written to (`rx` / outputs),
//! +2 per `SubDevice` read from (`tx` / inputs). Canonical totals: 0
//! (no PDOs / coupler),
//! 1 (Rx only), 2 (Tx only), 3 (both). See `ADR_0095` / `REQ_0828`.

/// One device per canonical case, in order: coupler (no PDOs → 0),
/// din (Tx only → 2), dout (Rx only → 1), combo (both → 3).
const FIXTURE: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler }
  - { label: din,  pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: dout, pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
  - { label: combo, pdos: { tx: [{ index: 0x6001, bit_offset: 0, bit_length: 8 }], rx: [{ index: 0x7001, bit_offset: 0, bit_length: 8 }] } }
channels: []
";

/// Generate source for `yaml` and pull the ordered list of `expected_wkc`
/// field values out of the `PDO_MAP` static via the AST.
fn generated_expected_wkcs(yaml: &str) -> Vec<u16> {
    let cfg = taktora_ethercat_netcfg::parse(yaml).expect("fixture parses");
    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");
    let file = syn::parse_file(&src).expect("generated source is valid Rust");

    // Locate `static PDO_MAP`.
    let pdo_map = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Static(s) if s.ident == "PDO_MAP" => Some(s),
            _ => None,
        })
        .expect("generated source defines a `static PDO_MAP`");

    // The initializer is `&[ SubDeviceMap { .. }, .. ]`: a reference to an
    // array literal of struct literals.
    let array = match &*pdo_map.expr {
        syn::Expr::Reference(r) => match &*r.expr {
            syn::Expr::Array(a) => a,
            _ => panic!("PDO_MAP initializer is not an array reference"),
        },
        _ => panic!("PDO_MAP initializer is not a reference"),
    };

    array
        .elems
        .iter()
        .map(|elem| {
            let syn::Expr::Struct(sub_map) = elem else {
                panic!("PDO_MAP element is not a struct literal")
            };
            let wkc_field = sub_map
                .fields
                .iter()
                .find(|f| match &f.member {
                    syn::Member::Named(ident) => ident == "expected_wkc",
                    syn::Member::Unnamed(_) => false,
                })
                .expect("SubDeviceMap has an `expected_wkc` field");
            match &wkc_field.expr {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(int),
                    ..
                }) => int
                    .base10_parse::<u16>()
                    .expect("expected_wkc literal fits in u16"),
                _ => panic!("expected_wkc field is not an integer literal"),
            }
        })
        .collect()
}

#[test]
fn derives_expected_wkc_from_pdo_directions() {
    // Validity check: generated source parses as Rust.
    let cfg = taktora_ethercat_netcfg::parse(FIXTURE).expect("fixture parses");
    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");
    assert!(
        syn::parse_file(&src).is_ok(),
        "generated source is valid Rust"
    );

    assert_eq!(
        generated_expected_wkcs(FIXTURE),
        vec![0, 2, 1, 3],
        "wkc = (rx ? 1 : 0) + (tx ? 2 : 0) per device, in bus order"
    );
}
