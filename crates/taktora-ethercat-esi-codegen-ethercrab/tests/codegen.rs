//! Integration test: parse the bullet-1 `el3001_like` fixture, run the full
//! codegen pipeline through [`EthercrabBackend`], and assert the emitted source
//! has the expected struct, identity constant, and `decode_inputs` reads.

use taktora_ethercat_esi_codegen::generate;
use taktora_ethercat_esi_codegen_ethercrab::EthercrabBackend;

/// Parse the fixture and pretty-print the generated module source.
fn generated_source() -> String {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/el3001_like.xml");
    source_from_xml(xml)
}

/// Parse arbitrary ESI XML and pretty-print the generated module source.
fn source_from_xml(xml: &str) -> String {
    let esi = taktora_ethercat_esi::parse(xml).expect("fixture parses");
    let tokens = generate(&esi, &EthercrabBackend).expect("codegen succeeds");
    let file = syn::parse2(tokens).expect("generated tokens parse as a Rust file");
    prettyplease::unparse(&file)
}

/// Whitespace-stripped form for substring assertions that should ignore
/// prettyplease's exact formatting.
fn squash(src: &str) -> String {
    src.chars().filter(|c| !c.is_whitespace()).collect()
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

// ---------------------------------------------------------------------------
// Multi-PDO devices: per-PDO sub-structs (T8).
// ---------------------------------------------------------------------------

/// EL2004 is an OUTPUT device with 4 `RxPdos` ("Channel 1".."Channel 4"), each a
/// single BOOL `Output` entry. Without sub-structs the four `output` fields
/// would collide; with sub-structs each channel owns its own `output`.
#[test]
fn el2004_emits_per_channel_substructs() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/beckhoff_el2004.xml");
    let src = source_from_xml(xml);

    // Device struct plus four channel sub-structs.
    assert!(
        src.contains("pub struct EL2004"),
        "missing device struct:\n{src}"
    );
    for n in 1..=4 {
        assert!(
            src.contains(&format!("pub struct EL2004Channel{n}")),
            "missing channel {n} sub-struct:\n{src}"
        );
    }

    // Each channel sub-struct holds its own bool output.
    let sq = squash(&src);
    assert!(
        sq.matches("puboutput:bool").count() >= 4,
        "expected four `pub output: bool` fields:\n{src}"
    );

    // Device struct gets one field per channel PDO.
    for n in 1..=4 {
        assert!(
            sq.contains(&format!("pubchannel_{n}:EL2004Channel{n}")),
            "missing device field channel_{n}:\n{src}"
        );
    }
}

/// EL2004's `encode_outputs` writes all four channel bits at running offsets
/// 0,1,2,3 spanning the four `RxPdos`, and reports lengths input=0, output=1.
#[test]
fn el2004_encode_spans_all_pdos() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/beckhoff_el2004.xml");
    let src = source_from_xml(xml);
    let sq = squash(&src);

    // Output device → input_len 0, output_len 1 (4 bits → 1 byte).
    assert!(
        sq.contains("fninput_len(&self)->usize{0usize}"),
        "input_len should be 0:\n{src}"
    );
    assert!(
        sq.contains("fnoutput_len(&self)->usize{1usize}"),
        "output_len should be 1:\n{src}"
    );

    // Running bit offsets 0..3 across the four channel PDOs.
    assert!(
        sq.contains("bits.set(0usize,self.channel_1.output)"),
        "missing channel_1 set@0:\n{src}"
    );
    assert!(
        sq.contains("bits.set(1usize,self.channel_2.output)"),
        "missing channel_2 set@1:\n{src}"
    );
    assert!(
        sq.contains("bits.set(2usize,self.channel_3.output)"),
        "missing channel_3 set@2:\n{src}"
    );
    assert!(
        sq.contains("bits.set(3usize,self.channel_4.output)"),
        "missing channel_4 set@3:\n{src}"
    );

    // Output length guard present.
    assert!(
        sq.contains("constNEED:usize=4usize"),
        "missing 4-bit output guard:\n{src}"
    );
}

/// EL3602 has 2 `TxPdos` ("AI Inputs Channel 1"/"...2"), each with BOOL/BIT2/
/// 7-bit pad/BOOL/DINT entries. Two sub-structs, BIT2 maps to u8, and the
/// second channel's decode offsets continue after the first.
#[test]
fn el3602_emits_two_channels_with_running_offsets() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/beckhoff_el3602.xml");
    let src = source_from_xml(xml);
    let sq = squash(&src);

    // Two channel sub-structs.
    assert!(
        src.contains("pub struct EL3602AiInputsChannel1"),
        "missing channel 1 sub-struct:\n{src}"
    );
    assert!(
        src.contains("pub struct EL3602AiInputsChannel2"),
        "missing channel 2 sub-struct:\n{src}"
    );

    // BIT2 (Limit 1) maps to u8 (2-bit sub-byte field).
    assert!(
        sq.contains("publimit_1:u8"),
        "BIT2 should map to u8:\n{src}"
    );
    // DINT (Value) maps to i32.
    assert!(
        sq.contains("pubvalue:i32"),
        "DINT should map to i32:\n{src}"
    );
    // BOOL entries.
    assert!(
        sq.contains("pubunderrange:bool"),
        "missing underrange bool:\n{src}"
    );

    // Each channel PDO is one TxPdo of width:
    //   Underrange(1) + Overrange(1) + Limit1(2) + Limit2(2) + Error(1)
    //   + pad(7) + TxPDO State(1) + TxPDO Toggle(1) + Value(32) = 48 bits.
    // Channel 1 occupies bits 0..48; channel 2's `Underrange` therefore reads
    // bit 48 (running offset spans both PDOs), and its `Value` loads 64..96.
    assert!(
        sq.contains("self.ai_inputs_channel_1.underrange=bits[0usize]"),
        "channel 1 underrange should read bit 0:\n{src}"
    );
    assert!(
        sq.contains("self.ai_inputs_channel_2.underrange=bits[48usize]"),
        "channel 2 underrange should read bit 48 (after channel 1):\n{src}"
    );
    assert!(
        sq.contains("self.ai_inputs_channel_2.value=bits[64usize..96usize].load_le::<i32>()"),
        "channel 2 value should load 64..96:\n{src}"
    );

    // 96-bit total input guard (2 x 48), input_len 12 bytes, no outputs.
    assert!(
        sq.contains("constNEED:usize=96usize"),
        "missing 96-bit input guard:\n{src}"
    );
    assert!(
        sq.contains("fninput_len(&self)->usize{12usize}"),
        "input_len should be 12:\n{src}"
    );
    assert!(
        sq.contains("fnoutput_len(&self)->usize{0usize}"),
        "output_len should be 0:\n{src}"
    );
}

/// Regression: a device with exactly one input PDO (and zero output PDOs) keeps
/// the FLAT shape — no sub-struct, fields directly on the device struct.
#[test]
fn single_pdo_device_stays_flat() {
    let src = generated_source();
    // No sub-struct type emitted.
    assert!(
        !src.contains("pub struct EL3001_likeTxPdo")
            && !src.contains("pub struct EL3001_likePdo")
            && !src.contains("pub struct EL3001_likeAi"),
        "single-PDO device must not emit a sub-struct:\n{src}"
    );
    // Fields live directly on the device struct.
    let sq = squash(&src);
    assert!(
        sq.contains("pubunderrange:bool") && sq.contains("pubvalue:i16"),
        "flat fields must remain on the device struct:\n{src}"
    );
}
