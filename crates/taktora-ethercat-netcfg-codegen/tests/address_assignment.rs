//! `TEST_0836` — each generated `SubDeviceMap.address` is the configured
//! station address: `0x1000 + n` by bus position, unless the device carries
//! an explicit `address:` override (then that value verbatim).

/// Three devices, no overrides → positional addresses `0x1000..`.
const FIXTURE_A: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: din,     pdos: { tx: [{ index: 0x6001, bit_offset: 0, bit_length: 8 }] } }
  - { label: dout,    pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels: []
";

/// Same bus, but the middle device pins its address with `address: 0x1005`.
const FIXTURE_B: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: din,     address: 0x1005, pdos: { tx: [{ index: 0x6001, bit_offset: 0, bit_length: 8 }] } }
  - { label: dout,    pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels: []
";

/// Generate source for `yaml` and pull the ordered list of `address` field
/// values out of the `PDO_MAP` static via the AST.
fn generated_addresses(yaml: &str) -> Vec<u16> {
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
            let address_field = sub_map
                .fields
                .iter()
                .find(|f| match &f.member {
                    syn::Member::Named(ident) => ident == "address",
                    syn::Member::Unnamed(_) => false,
                })
                .expect("SubDeviceMap has an `address` field");
            match &address_field.expr {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(int),
                    ..
                }) => int
                    .base10_parse::<u16>()
                    .expect("address literal fits in u16"),
                _ => panic!("address field is not an integer literal"),
            }
        })
        .collect()
}

#[test]
fn assigns_positional_and_override_station_addresses() {
    assert_eq!(
        generated_addresses(FIXTURE_A),
        vec![0x1000, 0x1001, 0x1002],
        "no overrides → positional 0x1000 + index"
    );
    assert_eq!(
        generated_addresses(FIXTURE_B),
        vec![0x1000, 0x1005, 0x1002],
        "middle device's address override is used verbatim"
    );
}
