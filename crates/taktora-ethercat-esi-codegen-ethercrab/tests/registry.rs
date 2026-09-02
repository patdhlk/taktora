//! `TEST_0421` — generated registry covers every emitted device.
//! Integration test for the device REGISTRY emitted by
//! [`EthercrabBackend::emit_module_root`] (`REQ_0525`): a static table mapping
//! each device's `Identity` to a `fn() -> Box<dyn EsiDevice>` factory, plus a
//! `device_for` lookup helper.

use taktora_ethercat_esi_codegen::generate;
use taktora_ethercat_esi_codegen_ethercrab::EthercrabBackend;

/// Parse arbitrary ESI XML and pretty-print the generated module source.
fn source_from_xml(xml: &str) -> String {
    let esi = taktora_ethercat_esi::parse(xml).expect("fixture parses");
    let tokens = generate(&esi, &EthercrabBackend).expect("codegen succeeds");
    let file = syn::parse2(tokens).expect("generated tokens parse as a Rust file");
    prettyplease::unparse(&file)
}

/// Whitespace-stripped form for substring assertions that ignore prettyplease's
/// exact formatting.
fn squash(src: &str) -> String {
    src.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The multi-device fixture (two devices, A and B) yields a `REGISTRY` with one
/// entry per device — each keyed by the device's already-emitted identity const
/// and constructing a `Box<dyn EsiDevice>` from the device's `Default` — and a
/// `device_for` lookup helper.
#[test]
fn multi_device_emits_registry_with_one_entry_per_device() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/multi_device.xml");
    let src = source_from_xml(xml);
    let sq = squash(&src);

    assert!(
        sq.contains("pubstaticREGISTRY"),
        "missing REGISTRY static:\n{src}"
    );

    // The table type: slice of (Identity, fn() -> Box<dyn EsiDevice>).
    assert!(
        sq.contains(
            "&[(taktora_ethercat_esi_rt::Identity,fn()->Box<dyntaktora_ethercat_esi_rt::EsiDevice>,)]"
        ),
        "REGISTRY should be a slice of (Identity, factory fn):\n{src}"
    );

    // One entry per device, keyed by the const emitted by emit_device, and
    // constructing the device via Default.
    assert!(
        sq.contains("A_REV00000001,||")
            && sq.contains("Box::new(A::default())asBox<dyntaktora_ethercat_esi_rt::EsiDevice>"),
        "missing registry entry for device A:\n{src}"
    );
    assert!(
        sq.contains("B_REV00000001,||")
            && sq.contains("Box::new(B::default())asBox<dyntaktora_ethercat_esi_rt::EsiDevice>"),
        "missing registry entry for device B:\n{src}"
    );

    // The lookup helper.
    assert!(
        sq.contains("pubfndevice_for(identity:taktora_ethercat_esi_rt::Identity,)->Option<Box<dyntaktora_ethercat_esi_rt::EsiDevice>>"),
        "missing device_for lookup helper:\n{src}"
    );
    assert!(
        sq.contains("REGISTRY.iter().find(|(id,_)|*id==identity).map(|(_,make)|make())"),
        "device_for should find by identity and call the factory:\n{src}"
    );
}

/// Registry entries are emitted in the same order as the resolved device set
/// (deterministic): A before B for the multi-device fixture.
#[test]
fn registry_entries_are_in_deterministic_order() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/multi_device.xml");
    let src = source_from_xml(xml);
    let sq = squash(&src);

    let a = sq.find("A_REV00000001,||").expect("entry A present");
    let b = sq.find("B_REV00000001,||").expect("entry B present");
    assert!(a < b, "registry entry A should precede B:\n{src}");
}

/// A single-device fixture (the bullet-1 `el3001_like`) yields a `REGISTRY` with
/// exactly one entry and the `device_for` helper.
#[test]
fn single_device_emits_registry_with_one_entry() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/el3001_like.xml");
    let src = source_from_xml(xml);
    let sq = squash(&src);

    assert!(
        sq.contains("pubstaticREGISTRY"),
        "missing REGISTRY static:\n{src}"
    );
    // prettyplease may wrap a long closure body in braces, so assert the const
    // key and the factory coercion independently rather than as one literal.
    assert!(
        sq.contains("EL3001_LIKE_REV00100000,||"),
        "missing registry entry keyed by EL3001_like identity const:\n{src}"
    );
    assert!(
        sq.contains("Box::new(EL3001_like::default())asBox<dyntaktora_ethercat_esi_rt::EsiDevice>"),
        "missing EL3001_like factory coercion:\n{src}"
    );
    assert!(
        sq.contains("pubfndevice_for"),
        "missing device_for helper:\n{src}"
    );
}
