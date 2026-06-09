//! `TEST_0825`/`TEST_0828` (via ESI) — end-to-end integration guard for the
//! full **ESI → netcfg → codegen** chain.
//!
//! A real single-device ESI XML fixture (vendor `#x00000021`, product
//! `#x07500354`, rev `#x00000001`) with one `TxPDO` (`#x1a00`, one inner
//! entry of 8 bits) and one `RxPDO` (`#x1600`, one inner entry of 8 bits) is
//! written to a temp file, referenced from a `network.yaml` via `esi:`, parsed
//! by `taktora_ethercat_netcfg::parse`, and fed to
//! `taktora_ethercat_netcfg_codegen::generate`. The generated `PDO_MAP` and routing
//! consts must carry the ESI-resolved PDOs at PDO-mapping-object granularity:
//! both directions present → `expected_wkc == 3`, Tx PDO index `0x1a00`/8 bits,
//! Rx PDO index `0x1600`/8 bits.

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
        <Mailbox DataLinkLayer="true"><CoE SdoInfo="1" PdoAssign="1"/></Mailbox>
        <Sm StartAddress="#x1000" ControlByte="#x44" Enable="1">Outputs</Sm>
        <TxPdo Sm="3">
          <Index>#x1a00</Index>
          <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen></Entry>
        </TxPdo>
        <RxPdo Sm="0">
          <Index>#x1600</Index>
          <Entry><Index>#x7000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen></Entry>
        </RxPdo>
      </Device>
    </Devices>
  </Descriptions>
</EtherCATInfo>
"##;

/// Same single device but with NO `CoE` mailbox — a "simple terminal". Its PDO
/// assignment must NOT be emitted (writing 0x1C12/0x1C13 to a mailbox-less
/// device fails with `NoReadMailbox` on the bus); it keeps its fixed default
/// mapping. `expected_wkc` is still derived from the resolved PDO presence.
const ESI_XML_NO_MAILBOX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions>
    <Devices>
      <Device>
        <Type ProductCode="#x07500355" RevisionNo="#x00000001">SimpleTerm</Type>
        <Name>SimpleTerm</Name>
        <Sm StartAddress="#x1000" ControlByte="#x44" Enable="1">Outputs</Sm>
        <TxPdo Sm="3">
          <Index>#x1a00</Index>
          <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen></Entry>
        </TxPdo>
        <RxPdo Sm="0">
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

/// Pull a `SubDeviceMap::new(..)` positional argument by the field name
/// it corresponds to. Argument order mirrors the constructor signature:
/// `new(address, rx_pdos, tx_pdos, expected_wkc)`.
fn field<'a>(call: &'a syn::ExprCall, name: &str) -> &'a syn::Expr {
    let index = match name {
        "address" => 0,
        "rx_pdos" => 1,
        "tx_pdos" => 2,
        "expected_wkc" => 3,
        other => panic!("SubDeviceMap::new has no `{other}` argument"),
    };
    call.args
        .iter()
        .nth(index)
        .unwrap_or_else(|| panic!("SubDeviceMap::new is missing argument `{name}`"))
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
                lit_u64(struct_field(entry, "index")),
                lit_u64(struct_field(entry, "bit_length")),
            )
        })
        .collect()
}

/// Pull the named field expr out of a `PdoEntry { .. }` struct literal.
fn struct_field<'a>(s: &'a syn::ExprStruct, name: &str) -> &'a syn::Expr {
    &s.fields
        .iter()
        .find(|f| matches!(&f.member, syn::Member::Named(id) if id == name))
        .unwrap_or_else(|| panic!("struct literal has a `{name}` field"))
        .expr
}

/// All `SubDeviceMap::new(..)` constructor calls from the generated
/// `PDO_MAP`.
fn sub_device_maps(file: &syn::File) -> Vec<&syn::ExprCall> {
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
        .map(|elem| {
            // An rx-carrying device emits
            // `SubDeviceMap::new(..).with_sm_watchdog(..)`; unwrap any
            // trailing method call to reach the `new(..)` constructor.
            let mut call = elem;
            while let syn::Expr::MethodCall(mc) = call {
                call = &mc.receiver;
            }
            match call {
                syn::Expr::Call(c) => c,
                _ => panic!("PDO_MAP element is not a `SubDeviceMap::new(..)` call"),
            }
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

    let cfg = taktora_ethercat_netcfg::parse(&yaml).expect("network.yaml + ESI parse");
    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");

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

    // tx_pdos carries one entry per TxPDO at PDO-mapping-object granularity:
    // the TxPDO mapping-object index #x1a00 with summed inner-entry bits = 8.
    assert_eq!(
        pdo_entries(field(map, "tx_pdos")),
        vec![(0x1a00, 8)],
        "tx_pdos carry the TxPDO mapping-object index 0x1a00 with bit_length 8"
    );

    // rx_pdos carries one entry per RxPDO at PDO-mapping-object granularity:
    // the RxPDO mapping-object index #x1600 with summed inner-entry bits = 8.
    assert_eq!(
        pdo_entries(field(map, "rx_pdos")),
        vec![(0x1600, 8)],
        "rx_pdos carry the RxPDO mapping-object index 0x1600 with bit_length 8"
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

#[test]
fn mailbox_less_device_emits_empty_assignment_but_keeps_wkc() {
    // A simple terminal (no CoE mailbox) must NOT receive PDO-assignment SDO
    // writes — its rx/tx in the generated SubDeviceMap are empty — yet it
    // still contributes its derived expected_wkc and routes process data.
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(ESI_XML_NO_MAILBOX.as_bytes())
        .expect("write ESI XML");
    let esi_path = esi.path().to_str().expect("ESI path is UTF-8");

    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - {{ label: term, esi: "{esi_path}" }}
channels: []
"#
    );

    let cfg = taktora_ethercat_netcfg::parse(&yaml).expect("network.yaml + ESI parse");
    let src = taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds");
    let file = syn::parse_file(&src).expect("generated source is valid Rust");

    let maps = sub_device_maps(&file);
    assert_eq!(maps.len(), 1, "single device → one SubDeviceMap");
    let map = maps[0];

    // WKC is still derived from the RESOLVED PDOs (both directions present).
    assert_eq!(
        lit_u64(field(map, "expected_wkc")),
        3,
        "mailbox-less device still contributes wkc = Rx(1) + Tx(2)"
    );

    // But the PDO-assignment lists are EMPTY — no 0x1C12/0x1C13 writes.
    assert_eq!(
        pdo_entries(field(map, "tx_pdos")),
        Vec::<(u64, u64)>::new(),
        "mailbox-less device emits no tx PDO assignment"
    );
    assert_eq!(
        pdo_entries(field(map, "rx_pdos")),
        Vec::<(u64, u64)>::new(),
        "mailbox-less device emits no rx PDO assignment"
    );
}
