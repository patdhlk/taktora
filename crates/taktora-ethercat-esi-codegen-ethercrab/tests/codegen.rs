//! Integration test: parse the bullet-1 `el3001_like` fixture, run the full
//! codegen pipeline through [`EthercrabBackend`], and assert the emitted source
//! has the expected struct, identity constant, and `decode_inputs` reads.

use taktora_ethercat_esi_codegen::generate;
use taktora_ethercat_esi_codegen_ethercrab::EthercrabBackend;

/// Parse the fixture and pretty-print the generated module source.
fn generated_source() -> String {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/el3001_like.xml");
    let esi = taktora_ethercat_esi::parse(xml).expect("fixture parses");
    let tokens = generate(&esi, &EthercrabBackend).expect("codegen succeeds");
    let file = syn::parse2(tokens).expect("generated tokens parse as a Rust file");
    prettyplease::unparse(&file)
}

#[test]
fn emits_device_struct_with_typed_fields() {
    let src = generated_source();

    assert!(
        src.contains("pub struct EL3001_like"),
        "missing device struct:\n{src}"
    );
    assert!(
        src.contains("pub underrange: bool"),
        "missing underrange field:\n{src}"
    );
    assert!(
        src.contains("pub value: i16"),
        "missing value field:\n{src}"
    );
}

#[test]
fn emits_identity_const_with_right_values() {
    let src = generated_source();

    assert!(
        src.contains("pub const EL3001_LIKE_REV00100000: taktora_ethercat_esi_rt::Identity"),
        "missing identity const:\n{src}"
    );
    // prettyplease keeps integer literals decimal with a `u32` suffix.
    assert!(
        src.contains("vendor_id: 2u32"),
        "missing vendor_id (0x2):\n{src}"
    );
    assert!(
        src.contains("product_code: 196685906u32"),
        "missing product_code (0x0bb93052 = 196685906):\n{src}"
    );
    assert!(
        src.contains("revision: 1048576u32"),
        "missing revision (0x00100000 = 1048576):\n{src}"
    );
}

#[test]
fn emits_decode_inputs_reads_and_guard() {
    let src = generated_source();

    // Length guard with the 24-bit total.
    assert!(
        src.contains("const NEED: usize = 24usize"),
        "missing 24-bit length guard:\n{src}"
    );
    assert!(
        src.contains("BufferTooShort"),
        "missing BufferTooShort guard:\n{src}"
    );

    // bool read of the first bit.
    assert!(
        src.contains("self.underrange = bits[0usize]"),
        "missing underrange bit-0 read:\n{src}"
    );

    // i16 load over 8..24 (the 7-bit pad between is skipped).
    let normalised = src.replace(' ', "");
    assert!(
        normalised.contains("self.value=bits[8usize..24usize].load_le::<i16>()"),
        "missing value load_le::<i16> over 8..24:\n{src}"
    );
}

#[test]
fn emits_lengths_and_noop_encode() {
    let src = generated_source();
    let normalised: String = src.chars().filter(|c| !c.is_whitespace()).collect();

    assert!(
        normalised.contains("fninput_len(&self)->usize{3usize}"),
        "input_len should be 3 bytes:\n{src}"
    );
    assert!(
        normalised.contains("fnoutput_len(&self)->usize{0usize}"),
        "output_len should be 0:\n{src}"
    );
    // No RxPdo → encode_outputs is a no-op returning Ok.
    assert!(
        src.contains("fn encode_outputs"),
        "missing encode_outputs:\n{src}"
    );
}
