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

// ---------------------------------------------------------------------------
// PDO assignment ALTERNATIVES: sum types (T9, REQ_0523/0524).
// ---------------------------------------------------------------------------

/// The synthetic ALT device has two non-fixed, non-mandatory `TxPdos` sharing
/// Sm=3 ("Standard" 16-bit, "Compact" 8-bit), no `<Exclude>` → one alternative
/// group of two. Codegen must emit a `<Dev>PdoAssignment` sum type, one
/// `<Dev>Pdo<Variant>` struct per alternative, an enum-typed `pdo` field, and a
/// manual `Default` picking the first variant.
fn alt_source() -> String {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/pdo_alternatives.xml");
    source_from_xml(xml)
}

#[test]
fn alt_emits_pdo_assignment_enum_with_one_variant_per_alternative() {
    let src = alt_source();
    let sq = squash(&src);

    // The sum type with one variant per alternative, each embedding its struct.
    assert!(
        src.contains("pub enum ALTPdoAssignment"),
        "missing ALTPdoAssignment enum:\n{src}"
    );
    assert!(
        sq.contains("Standard(ALTPdoStandard)"),
        "missing Standard variant embedding its struct:\n{src}"
    );
    assert!(
        sq.contains("Compact(ALTPdoCompact)"),
        "missing Compact variant embedding its struct:\n{src}"
    );
}

#[test]
fn alt_emits_per_variant_structs_with_typed_entries() {
    let src = alt_source();
    let sq = squash(&src);

    // Standard: a 16-bit entry → u16 field; Compact: an 8-bit entry → u8.
    assert!(
        src.contains("pub struct ALTPdoStandard"),
        "missing ALTPdoStandard struct:\n{src}"
    );
    assert!(
        src.contains("pub struct ALTPdoCompact"),
        "missing ALTPdoCompact struct:\n{src}"
    );
    // The shared entry name disambiguation is per-struct, so the field exists in
    // each. 16-bit untyped → u16; 8-bit untyped → u8.
    assert!(
        sq.contains(":u16"),
        "Standard variant should hold a u16 entry:\n{src}"
    );
    assert!(
        sq.contains(":u8"),
        "Compact variant should hold a u8 entry:\n{src}"
    );
}

#[test]
fn alt_device_field_is_the_enum_with_manual_default() {
    let src = alt_source();
    let sq = squash(&src);

    // The device struct carries the enum-typed `pdo` field.
    assert!(
        sq.contains("pubpdo:ALTPdoAssignment"),
        "missing enum-typed pdo field on device:\n{src}"
    );

    // Manual Default picks the first alternative (Standard).
    assert!(
        sq.contains("implDefaultforALTPdoAssignment"),
        "missing manual Default impl:\n{src}"
    );
    assert!(
        sq.contains("Self::Standard(Default::default())"),
        "Default should pick the first variant (Standard):\n{src}"
    );
}

#[test]
fn alt_decode_inputs_matches_active_variant() {
    let src = alt_source();
    let sq = squash(&src);

    // decode_inputs matches the enum field and reads the active variant's entry.
    assert!(
        sq.contains("match&mutself.pdo"),
        "decode_inputs should match the enum field:\n{src}"
    );
    assert!(
        sq.contains("ALTPdoAssignment::Standard("),
        "decode should have a Standard arm:\n{src}"
    );
    assert!(
        sq.contains("ALTPdoAssignment::Compact("),
        "decode should have a Compact arm:\n{src}"
    );
    // The active variant's entry is read at the running offset (0, no always-on
    // PDOs precede it in this direction).
    assert!(
        sq.contains("load_le::<u16>()") && sq.contains("load_le::<u8>()"),
        "each arm should load its own variant's entry:\n{src}"
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

/// The ALT alternatives module must be syntactically valid Rust. `source_from_xml`
/// already round-trips the tokens through `syn::parse2::<syn::File>`, so reaching
/// a non-empty pretty-printed string proves the emitted tokens parse. (A full
/// compile + decode check rides on the landing-pad crate, which compiles every
/// generated module via `build.rs`; the synthetic ALT device is covered here at
/// the token-validity level per the T9 scope.)
#[test]
fn alt_output_is_valid_rust() {
    let src = alt_source();
    assert!(!src.is_empty(), "ALT module should produce source");
    // Re-parse the pretty-printed output as a final guard.
    let _: syn::File = syn::parse_str(&src).expect("ALT module is valid Rust");
}

// ---------------------------------------------------------------------------
// Refined classification (the EL1262 unblock): a non-fixed/non-mandatory PDO
// that is the ONLY candidate in its sync-manager group is NOT an alternative;
// it is an always-on PDO whose mapping happens to be reconfigurable.
// ---------------------------------------------------------------------------

/// The EL1262 shape: two non-fixed/non-mandatory `TxPdo`s on DISTINCT sync
/// managers (Sm 3 and Sm 4), no `<Exclude>`. They coexist (one per channel) —
/// they are not alternatives of each other. Each is the lone candidate in its
/// Sm group, so BOTH must be reclassified as always-on (per-PDO sub-structs),
/// with NO `PdoAssignment` enum and NO `MultipleAlternativeGroups` error.
const EL1262_SHAPE_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000002</Id></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="#x00020000" RevisionNo="#x00000001">EL1262LIKE</Type>
      <TxPdo Sm="3" Fixed="0" Mandatory="0">
        <Index>#x1a00</Index>
        <Name>Channel 1</Name>
        <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>Value</Name></Entry>
      </TxPdo>
      <TxPdo Sm="4" Fixed="0" Mandatory="0">
        <Index>#x1a20</Index>
        <Name>Channel 2</Name>
        <Entry><Index>#x6010</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>Value</Name></Entry>
      </TxPdo>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"##;

#[test]
fn distinct_sm_singletons_emit_no_enum() {
    let src = source_from_xml(EL1262_SHAPE_XML);
    assert!(
        !src.contains("PdoAssignment"),
        "two lone candidates on distinct SMs must NOT produce an alternative enum:\n{src}"
    );
    assert!(
        !src.contains("enum "),
        "no choice enum should be emitted for coexisting singleton PDOs:\n{src}"
    );
}

#[test]
fn distinct_sm_singletons_become_per_pdo_substructs() {
    let src = source_from_xml(EL1262_SHAPE_XML);
    // Two always-on PDOs in one direction → per-PDO sub-structs (the T8 split
    // shape), one per channel, both fields living on the device struct.
    assert!(
        src.contains("pub struct EL1262LIKEChannel1")
            && src.contains("pub struct EL1262LIKEChannel2"),
        "both reclassified PDOs should become per-PDO sub-structs:\n{src}"
    );
    let sq = squash(&src);
    assert!(
        sq.contains("pubchannel_1:EL1262LIKEChannel1")
            && sq.contains("pubchannel_2:EL1262LIKEChannel2"),
        "device struct should carry both sub-struct fields:\n{src}"
    );
}

#[test]
fn distinct_sm_singletons_thread_running_offset() {
    // Channel 1 occupies bits 0..16; Channel 2 reads at the running offset 16.
    let src = source_from_xml(EL1262_SHAPE_XML);
    let sq = squash(&src);
    assert!(
        sq.contains("bits[0usize..16usize].load_le::<u16>()"),
        "channel 1 reads bits 0..16:\n{src}"
    );
    assert!(
        sq.contains("bits[16usize..32usize].load_le::<u16>()"),
        "channel 2 reads at the running offset 16..32:\n{src}"
    );
}

/// Two SEPARATE >=2-PDO alternative groups in one direction (Sm 3 has two
/// competing PDOs AND Sm 4 has two competing PDOs) is a genuinely-rare case
/// still rejected with `MultipleAlternativeGroups` (no multi-group offset
/// threading is implemented here).
const TWO_GENUINE_GROUPS_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000002</Id></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="#x00030000" RevisionNo="#x00000001">TWOGRP</Type>
      <TxPdo Sm="3" Fixed="0" Mandatory="0">
        <Index>#x1a00</Index><Name>A0</Name>
        <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>Value</Name></Entry>
      </TxPdo>
      <TxPdo Sm="3" Fixed="0" Mandatory="0">
        <Index>#x1a01</Index><Name>A1</Name>
        <Entry><Index>#x6000</Index><SubIndex>1</SubIndex><BitLen>8</BitLen><Name>Value</Name></Entry>
      </TxPdo>
      <TxPdo Sm="4" Fixed="0" Mandatory="0">
        <Index>#x1a10</Index><Name>B0</Name>
        <Entry><Index>#x6010</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>Value</Name></Entry>
      </TxPdo>
      <TxPdo Sm="4" Fixed="0" Mandatory="0">
        <Index>#x1a11</Index><Name>B1</Name>
        <Entry><Index>#x6010</Index><SubIndex>1</SubIndex><BitLen>8</BitLen><Name>Value</Name></Entry>
      </TxPdo>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"##;

#[test]
fn two_genuine_alternative_groups_are_still_rejected() {
    use taktora_ethercat_esi_codegen::CodegenError;
    let esi = taktora_ethercat_esi::parse(TWO_GENUINE_GROUPS_XML).expect("fixture parses");
    let err = generate(&esi, &EthercrabBackend)
        .expect_err("two genuine alternative groups in one direction must be rejected");
    assert!(
        matches!(
            err,
            CodegenError::MultipleAlternativeGroups {
                direction: "Tx",
                ..
            }
        ),
        "expected MultipleAlternativeGroups for Tx, got {err:?}"
    );
}
