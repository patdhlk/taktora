//! `REQ_0838` — `generate` emits a self-contained per-device identity table
//! (`struct ExpectedIdentity` + `static EXPECTED_IDENTITIES`) for a future
//! runtime bring-up check. One entry per device whose `identity` is `Some`;
//! devices with no known identity contribute no entry.

use std::io::Write as _;

/// A single-device ESI: identity vendor `#x21`, product `#x07500354`, rev `1`.
const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000001">Coupler</Type>
    <Name>Coupler</Name>
    <TxPdo>
      <Index>#x1a00</Index>
      <Entry><Index>#x6000</Index><BitLen>8</BitLen></Entry>
    </TxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>
"##;

/// An `Expr` that is an integer literal → its `u64` value.
fn lit_u64(expr: &syn::Expr) -> u64 {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(int),
            ..
        }) => int.base10_parse::<u64>().expect("integer literal"),
        _ => panic!("expected an integer literal"),
    }
}

/// Pull the named field expr out of a struct literal.
fn field<'a>(s: &'a syn::ExprStruct, name: &str) -> &'a syn::Expr {
    &s.fields
        .iter()
        .find(|f| matches!(&f.member, syn::Member::Named(id) if id == name))
        .unwrap_or_else(|| panic!("struct literal has a `{name}` field"))
        .expr
}

/// The `Some(x)` inner integer literal of a `station_alias` field, or `None`.
fn opt_u64(expr: &syn::Expr) -> Option<u64> {
    match expr {
        syn::Expr::Call(call) => {
            // Some(..)
            let syn::Expr::Path(p) = &*call.func else {
                panic!("station_alias is not a Some(..)/None path")
            };
            assert_eq!(
                p.path.segments.last().expect("non-empty path").ident,
                "Some",
                "station_alias call is not Some(..)"
            );
            Some(lit_u64(&call.args[0]))
        }
        syn::Expr::Path(p) => {
            assert_eq!(
                p.path.segments.last().expect("non-empty path").ident,
                "None",
                "bare station_alias path is not None"
            );
            None
        }
        _ => panic!("station_alias is neither Some(..) nor None"),
    }
}

#[test]
fn emits_expected_identity_table_for_devices_with_identity() {
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(ESI_XML.as_bytes()).expect("write ESI XML");
    let esi_path = esi.path().to_str().expect("ESI path is UTF-8");

    // One esi device (identity from ESI, station_alias 7) and one inline
    // device with no identity.
    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - label: coupler
    esi: "{esi_path}"
    station_alias: 7
  - label: io
    pdos:
      tx:
        - {{ index: 0x6010, bit_offset: 0, bit_length: 8 }}
channels: []
"#
    );

    let cfg = taktora_ethercat_netcfg::parse(&yaml).expect("network.yaml + ESI parse");
    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");

    let file = syn::parse_file(&src).expect("generated source is valid Rust");

    // Defines `struct ExpectedIdentity`.
    assert!(
        file.items.iter().any(|item| matches!(
            item,
            syn::Item::Struct(s) if s.ident == "ExpectedIdentity"
        )),
        "generated source defines `struct ExpectedIdentity`"
    );

    // Defines `static EXPECTED_IDENTITIES`.
    let table = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Static(s) if s.ident == "EXPECTED_IDENTITIES" => Some(s),
            _ => None,
        })
        .expect("generated source defines a `static EXPECTED_IDENTITIES`");

    let array = match &*table.expr {
        syn::Expr::Reference(r) => match &*r.expr {
            syn::Expr::Array(a) => a,
            _ => panic!("EXPECTED_IDENTITIES initializer is not an array reference"),
        },
        _ => panic!("EXPECTED_IDENTITIES initializer is not a reference"),
    };

    // Exactly one entry: the esi device (the inline device has no identity).
    assert_eq!(
        array.elems.len(),
        1,
        "only the device with an identity contributes an entry"
    );

    let syn::Expr::Struct(entry) = &array.elems[0] else {
        panic!("EXPECTED_IDENTITIES element is not a struct literal")
    };

    assert_eq!(lit_u64(field(entry, "address")), 0x1000, "address");
    assert_eq!(lit_u64(field(entry, "vendor_id")), 0x21, "vendor_id");
    assert_eq!(
        lit_u64(field(entry, "product_code")),
        0x0750_0354,
        "product_code"
    );
    assert_eq!(lit_u64(field(entry, "revision")), 1, "revision");
    assert_eq!(
        opt_u64(field(entry, "station_alias")),
        Some(7),
        "station_alias"
    );
}
