//! Integration tests for the ESI codegen layer: collision policy (`REQ_0512`)
//! and the `generate` orchestration over a parsed fixture.

use std::cell::RefCell;

use proc_macro2::TokenStream;
use quote::quote;
use taktora_ethercat_esi as esi;
use taktora_ethercat_esi_codegen::{
    CodegenBackend, CodegenError, Device, generate, pdo_assignment_enum_ident,
    pdo_assignment_field_ident, pdo_variant_ident, pdo_variant_struct_ident,
};

#[test]
fn pdo_assignment_idents_for_single_and_multi_group() {
    let dev = proc_macro2::Ident::new("ALT", proc_macro2::Span::call_site());

    // Single group: bare `<Dev>PdoAssignment` enum and a `pdo` field.
    assert_eq!(
        pdo_assignment_enum_ident(&dev, None).unwrap().to_string(),
        "ALTPdoAssignment"
    );
    assert_eq!(pdo_assignment_field_ident(None).unwrap().to_string(), "pdo");

    // Multi-group: label-disambiguated enum + field.
    assert_eq!(
        pdo_assignment_enum_ident(&dev, Some("Sm3"))
            .unwrap()
            .to_string(),
        "ALTPdoAssignmentSm3"
    );
    assert_eq!(
        pdo_assignment_field_ident(Some("sm3")).unwrap().to_string(),
        "pdo_sm3"
    );
}

#[test]
fn pdo_variant_idents_from_name_and_index() {
    let dev = proc_macro2::Ident::new("ALT", proc_macro2::Span::call_site());

    assert_eq!(
        pdo_variant_ident(Some("Standard"), 0x1A00)
            .unwrap()
            .to_string(),
        "Standard"
    );
    assert_eq!(
        pdo_variant_ident(Some("Compact"), 0x1A01)
            .unwrap()
            .to_string(),
        "Compact"
    );
    // Unnamed → index fallback.
    assert_eq!(
        pdo_variant_ident(None, 0x1A02).unwrap().to_string(),
        "Pdo1a02"
    );

    // Per-variant struct ident is `<Dev>Pdo<Variant>`.
    assert_eq!(
        pdo_variant_struct_ident(&dev, Some("Standard"), 0x1A00)
            .unwrap()
            .to_string(),
        "ALTPdoStandard"
    );
    assert_eq!(
        pdo_variant_struct_ident(&dev, Some("Compact"), 0x1A01)
            .unwrap()
            .to_string(),
        "ALTPdoCompact"
    );
}

/// A no-op backend that records the resolved struct idents it saw, so tests can
/// assert ordering and the module-root call without inspecting emitted tokens.
#[derive(Default)]
struct RecordingBackend {
    devices_seen: RefCell<Vec<String>>,
    roots_seen: RefCell<Vec<usize>>,
}

impl CodegenBackend for RecordingBackend {
    fn emit_device(&self, device: &Device) -> Result<TokenStream, CodegenError> {
        self.devices_seen
            .borrow_mut()
            .push(device.struct_ident.to_string());
        let ident = &device.struct_ident;
        Ok(quote! { pub struct #ident; })
    }

    fn emit_module_root(&self, devices: &[Device]) -> Result<TokenStream, CodegenError> {
        self.roots_seen.borrow_mut().push(devices.len());
        Ok(TokenStream::new())
    }
}

/// Two devices that collide on a shared base ident (`EL3204`) but differ by
/// revision. Returns the ESI XML with the devices in the given order.
fn colliding_esi(reversed: bool) -> String {
    let dev_a = r##"<Device><Type ProductCode="#x00000100" RevisionNo="#x00100000">EL3204</Type></Device>"##;
    let dev_b = r##"<Device><Type ProductCode="#x00000100" RevisionNo="#x00110000">EL3204</Type></Device>"##;
    let dev_c = r##"<Device><Type ProductCode="#x00000200" RevisionNo="#x00100000">EL5001</Type></Device>"##;
    let (first, second) = if reversed {
        (dev_b, dev_a)
    } else {
        (dev_a, dev_b)
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000002</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    {first}
    {second}
    {dev_c}
  </Devices></Descriptions>
</EtherCATInfo>"#
    )
}

#[test]
fn colliding_devices_get_revision_suffixed_struct_idents() {
    let file = esi::parse(&colliding_esi(false)).expect("parse");
    let backend = RecordingBackend::default();
    generate(&file, &backend).expect("generate");

    let seen = backend.devices_seen.borrow();
    assert_eq!(
        *seen,
        vec![
            "EL3204_REV00100000".to_owned(),
            "EL3204_REV00110000".to_owned(),
            // Non-colliding device keeps its bare sanitised ident.
            "EL5001".to_owned(),
        ]
    );
}

#[test]
fn collision_resolution_is_order_independent() {
    let forward = esi::parse(&colliding_esi(false)).expect("parse forward");
    let reversed = esi::parse(&colliding_esi(true)).expect("parse reversed");

    let resolve = |file: &esi::EsiFile| {
        let backend = RecordingBackend::default();
        generate(file, &backend).expect("generate");
        let mut seen = backend.devices_seen.borrow().clone();
        seen.sort();
        seen
    };

    // Same resolved idents regardless of input order.
    assert_eq!(resolve(&forward), resolve(&reversed));
    assert_eq!(
        resolve(&forward),
        vec![
            "EL3204_REV00100000".to_owned(),
            "EL3204_REV00110000".to_owned(),
            "EL5001".to_owned(),
        ]
    );
}

/// A backend that captures the first resolved device's fields for assertion.
#[derive(Default)]
struct Capture {
    struct_ident: RefCell<String>,
    const_ident: RefCell<String>,
    identity: RefCell<Option<taktora_ethercat_esi_codegen::Identity>>,
}

impl CodegenBackend for Capture {
    fn emit_device(&self, device: &Device) -> Result<TokenStream, CodegenError> {
        *self.struct_ident.borrow_mut() = device.struct_ident.to_string();
        *self.const_ident.borrow_mut() = device.const_ident.to_string();
        *self.identity.borrow_mut() = Some(device.identity);
        Ok(TokenStream::new())
    }
    fn emit_module_root(&self, _devices: &[Device]) -> Result<TokenStream, CodegenError> {
        Ok(TokenStream::new())
    }
}

#[test]
fn generate_over_el3001_fixture_resolves_expected_idents() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/el3001_like.xml");
    let file = esi::parse(xml).expect("parse el3001");

    let devices = {
        // Resolve via generate + a recording backend; assert single device.
        let backend = RecordingBackend::default();
        generate(&file, &backend).expect("generate");
        assert_eq!(*backend.roots_seen.borrow(), vec![1]);
        backend.devices_seen.borrow().clone()
    };

    // <Type> is "EL3001-like" → sanitised "EL3001_like"; single device, no
    // collision, so the struct ident is bare.
    assert_eq!(devices, vec!["EL3001_like".to_owned()]);

    // Inspect the resolved Device fields directly via a capturing backend.
    let cap = Capture::default();
    generate(&file, &cap).expect("generate capture");

    assert_eq!(*cap.struct_ident.borrow(), "EL3001_like");
    assert_eq!(*cap.const_ident.borrow(), "EL3001_LIKE_REV00100000");

    let identity = cap.identity.borrow().expect("identity captured");
    assert_eq!(identity.vendor_id, 0x0000_0002);
    assert_eq!(identity.product_code, 0x0bb9_3052);
    assert_eq!(identity.revision, 0x0010_0000);
}

#[test]
fn generated_tokens_parse_as_rust() {
    let file = esi::parse(&colliding_esi(false)).expect("parse");
    let backend = RecordingBackend::default();
    let ts = generate(&file, &backend).expect("generate");
    // The emitted token stream must be a syntactically valid Rust file.
    let _file: syn::File = syn::parse2(ts).expect("emitted tokens parse as a Rust file");
}

/// A device whose `<Type>` sanitises to a Rust keyword (`match`) must resolve to
/// a keyword-escaped struct ident (`match_`) so that `pub struct #ident;` is
/// valid Rust rather than a parse error.
#[test]
fn keyword_type_resolves_to_escaped_struct_ident() {
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000002</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    <Device><Type ProductCode="#x00000100" RevisionNo="#x00100000">match</Type></Device>
  </Devices></Descriptions>
</EtherCATInfo>"##;
    let file = esi::parse(xml).expect("parse keyword device");

    let cap = Capture::default();
    generate(&file, &cap).expect("generate capture");
    assert_eq!(*cap.struct_ident.borrow(), "match_");
    // The keyword guard runs uniformly in `sanitise_ident`, so the base ident
    // carries the escape underscore before uppercasing: `match_` → `MATCH_`,
    // then the `_REV..` suffix is appended.
    assert_eq!(*cap.const_ident.borrow(), "MATCH__REV00100000");

    // The emitted `pub struct match_;` must be valid Rust.
    let backend = RecordingBackend::default();
    let ts = generate(&file, &backend).expect("generate");
    let _file: syn::File = syn::parse2(ts).expect("keyword-escaped tokens parse as a Rust file");
}
