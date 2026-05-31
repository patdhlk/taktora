//! `TEST_0825`/`TEST_0828` (via ESI) — end-to-end integration guard for the
//! full **ESI → netcfg → codegen** chain.
//!
//! A real single-device ESI XML fixture (vendor `#x00000021`, product
//! `#x07500354`, rev `#x00000001`) with one `TxPDO` entry (`#x6000`/8) and one
//! `RxPDO` entry (`#x7000`/8) is written to a temp file, referenced from a
//! `network.yaml` via `esi:`, parsed by `ethercat_netcfg::parse`, and fed to
//! `ethercat_netcfg_codegen::generate`. The generated `PDO_MAP` and routing
//! consts must carry the ESI-resolved PDOs verbatim: both directions present
//! → `expected_wkc == 3`, Tx entry `0x6000`, Rx entry `0x7000`.

use std::io::Write as _;

/// A single-device ESI: both directions populated so the device's
/// derived `expected_wkc` is 3 (Rx +1, Tx +2).
const ESI_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions>
    <Devices>
      <Device>
        <Type ProductCode="#x07500354" RevisionNo="#x00000001">Coupler</Type>
        <Name>Coupler</Name>
        <TxPdo>
          <Index>#x1a00</Index>
          <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen></Entry>
        </TxPdo>
        <RxPdo>
          <Index>#x1600</Index>
          <Entry><Index>#x7000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen></Entry>
        </RxPdo>
      </Device>
    </Devices>
  </Descriptions>
</EtherCATInfo>
"##;

/// An `Expr` that is an integer literal → its `u64` value (helper for both
/// `address`/`expected_wkc`/`index`/`bit_length` field extraction).
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

/// Extract the array of `PdoEntry { index, bit_length, .. }` from a
/// `&[ .. ]` field expr, returning `(index, bit_length)` per entry.
fn pdo_entries(expr: &syn::Expr) -> Vec<(u64, u64)> {
    let syn::Expr::Reference(r) = expr else {
        panic!("PDO field is not a reference")
    };
    let syn::Expr::Array(arr) = &*r.expr else {
        panic!("PDO field is not an array reference")
    };
    arr.elems
        .iter()
        .map(|e| {
            let syn::Expr::Struct(entry) = e else {
                panic!("PDO array element is not a struct literal")
            };
            (
                lit_u64(field(entry, "index")),
                lit_u64(field(entry, "bit_length")),
            )
        })
        .collect()
}

/// All `SubDeviceMap { .. }` struct literals from the generated `PDO_MAP`.
fn sub_device_maps(file: &syn::File) -> Vec<&syn::ExprStruct> {
    let pdo_map = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Static(s) if s.ident == "PDO_MAP" => Some(s),
            _ => None,
        })
        .expect("generated source defines a `static PDO_MAP`");

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
        .map(|elem| match elem {
            syn::Expr::Struct(s) => s,
            _ => panic!("PDO_MAP element is not a struct literal"),
        })
        .collect()
}

/// The `EthercatRouting::new(address, direction, ..)` const init for a routing
/// `pub const <NAME>`, returning `(address, direction_string)`.
fn routing_const(file: &syn::File, name: &str) -> (u64, String) {
    let konst = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(c) if c.ident == name => Some(c),
            _ => None,
        })
        .unwrap_or_else(|| panic!("generated source defines `pub const {name}`"));

    let syn::Expr::Call(call) = &*konst.expr else {
        panic!("{name} initializer is not a call")
    };
    let address = lit_u64(&call.args[0]);
    let direction = match &call.args[1] {
        syn::Expr::Path(p) => p
            .path
            .segments
            .last()
            .expect("non-empty path")
            .ident
            .to_string(),
        _ => panic!("direction arg is not a path"),
    };
    (address, direction)
}

#[test]
fn emits_correct_tables_from_esi_resolved_pdos() {
    // Write the ESI fixture to a temp file and interpolate its absolute path
    // into the network.yaml (netcfg resolves esi paths against the CWD).
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(ESI_XML.as_bytes()).expect("write ESI XML");
    let esi_path = esi.path().to_str().expect("ESI path is UTF-8");

    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - {{ label: coupler, esi: "{esi_path}" }}
channels:
  - {{ name: "ethercat.coupler.in",  device: coupler, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }}
  - {{ name: "ethercat.coupler.out", device: coupler, direction: rx, bit_offset: 0, bit_length: 8, element_type: u8 }}
"#
    );

    let cfg = ethercat_netcfg::parse(&yaml).expect("network.yaml + ESI parse");
    let src = ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");

    let file = syn::parse_file(&src).expect("generated source is valid Rust");

    // Exactly one SubDeviceMap.
    let maps = sub_device_maps(&file);
    assert_eq!(maps.len(), 1, "single device → one SubDeviceMap");
    let map = maps[0];

    // Address is the positional default 0x1000.
    assert_eq!(lit_u64(field(map, "address")), 0x1000, "station address");

    // Both directions present → expected_wkc == 3.
    assert_eq!(
        lit_u64(field(map, "expected_wkc")),
        3,
        "both ESI directions present → wkc = Rx(1) + Tx(2)"
    );

    // tx_pdos carries the ESI TxPDO entry (#x6000 = 24576, BitLen 8).
    assert_eq!(
        pdo_entries(field(map, "tx_pdos")),
        vec![(0x6000, 8)],
        "tx_pdos carry the ESI TxPDO entry (index 0x6000/24576, bit_length 8)"
    );

    // rx_pdos carries the ESI RxPDO entry (#x7000 = 28672, BitLen 8).
    assert_eq!(
        pdo_entries(field(map, "rx_pdos")),
        vec![(0x7000, 8)],
        "rx_pdos carry the ESI RxPDO entry (index 0x7000/28672, bit_length 8)"
    );

    // Routing consts: both at address 0x1000, correct directions.
    assert_eq!(
        routing_const(&file, "ETHERCAT_COUPLER_IN"),
        (0x1000, "Tx".to_owned()),
        "tx channel routes to address 0x1000 with direction Tx"
    );
    assert_eq!(
        routing_const(&file, "ETHERCAT_COUPLER_OUT"),
        (0x1000, "Rx".to_owned()),
        "rx channel routes to address 0x1000 with direction Rx"
    );
}
