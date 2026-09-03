//! `TEST_0420` — EL3001 backend output: one device struct and one identity const per device.
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
fn emits_device_struct_with_op_mode_field() {
    let src = generated_source();
    let sq = squash(&src);

    // The device struct is now a thin wrapper over the OpMode enum.
    assert!(
        sq.contains("pubstructEL3001_like{pubmode:EL3001_likeOpMode,}"),
        "device struct should carry a single `mode: <Dev>OpMode` field:\n{src}"
    );

    // The single resolved assignment lives under the `Default` variant's
    // `inputs` direction struct.
    assert!(
        src.contains("pub struct EL3001_likeDefaultIn"),
        "missing Default-variant input direction struct:\n{src}"
    );
    assert!(
        sq.contains("pubunderrange:bool"),
        "missing underrange field:\n{src}"
    );
    assert!(sq.contains("pubvalue:i16"), "missing value field:\n{src}");
}

#[test]
fn emits_op_mode_enum_with_single_default_variant() {
    let src = generated_source();
    let sq = squash(&src);

    // No <AlternativeSmMapping> → one synthetic `Default` variant holding the
    // `<Dev>Default` data struct.
    assert!(
        src.contains("pub enum EL3001_likeOpMode"),
        "missing OpMode enum:\n{src}"
    );
    assert!(
        sq.contains("Default(EL3001_likeDefault),"),
        "OpMode should have exactly the Default(<Dev>Default) variant:\n{src}"
    );
    // Exactly one variant: the enum body holds a single `Default(..)`.
    assert_eq!(
        sq.matches("Default(EL3001_likeDefault)").count(),
        1,
        "expected exactly one variant:\n{src}"
    );

    // Manual Default impl selects the first (only) variant.
    assert!(
        sq.contains("implDefaultforEL3001_likeOpMode"),
        "missing manual Default impl:\n{src}"
    );
    assert!(
        sq.contains("Self::Default(Default::default())"),
        "Default should select the first variant:\n{src}"
    );

    // The variant data struct groups the two direction structs.
    assert!(
        sq.contains("pubstructEL3001_likeDefault{pubinputs:EL3001_likeDefaultIn,puboutputs:EL3001_likeDefaultOut,}"),
        "variant struct should hold `inputs`/`outputs` direction structs:\n{src}"
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

    // decode_inputs now matches the active mode and reads through the
    // match-bound `m.inputs` path.
    let sq = squash(&src);
    assert!(
        sq.contains("match&mutself.mode"),
        "decode_inputs should match the active mode:\n{src}"
    );
    assert!(
        sq.contains("EL3001_likeOpMode::Default(m)=>"),
        "decode should have a Default arm:\n{src}"
    );

    // bool read of the first bit.
    assert!(
        sq.contains("m.inputs.underrange=bits[0usize]"),
        "missing underrange bit-0 read:\n{src}"
    );

    // i16 load over 8..24 (the 7-bit pad between is skipped).
    assert!(
        sq.contains("m.inputs.value=bits[8usize..24usize].load_le::<i16>()"),
        "missing value load_le::<i16> over 8..24:\n{src}"
    );
}

#[test]
fn emits_lengths_and_noop_encode() {
    let src = generated_source();
    let sq = squash(&src);

    // input_len/output_len are now per-mode matches; the single Default arm
    // reports 3 input bytes (24 bits) and 0 output bytes.
    assert!(
        sq.contains(
            "fninput_len(&self)->usize{match&self.mode{EL3001_likeOpMode::Default(_)=>3usize,}}"
        ),
        "input_len Default arm should be 3 bytes:\n{src}"
    );
    assert!(
        sq.contains(
            "fnoutput_len(&self)->usize{match&self.mode{EL3001_likeOpMode::Default(_)=>0usize,}}"
        ),
        "output_len Default arm should be 0:\n{src}"
    );
    // No RxPdo → encode_outputs writes nothing in the Default arm.
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
/// would collide; with sub-structs each channel owns its own `output`. Under the
/// joint-OpMode model the four channels live under the single `Default` variant's
/// `outputs` direction struct.
#[test]
fn el2004_emits_per_channel_substructs() {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/beckhoff_el2004.xml");
    let src = source_from_xml(xml);

    // Device struct (mode wrapper) plus four channel sub-structs under Default.
    assert!(
        src.contains("pub struct EL2004 {"),
        "missing device struct:\n{src}"
    );
    for n in 1..=4 {
        assert!(
            src.contains(&format!("pub struct EL2004DefaultChannel{n}")),
            "missing channel {n} sub-struct:\n{src}"
        );
    }

    // Each channel sub-struct holds its own bool output.
    let sq = squash(&src);
    assert!(
        sq.matches("puboutput:bool").count() >= 4,
        "expected four `pub output: bool` fields:\n{src}"
    );

    // The Default `outputs` direction struct gets one field per channel PDO.
    for n in 1..=4 {
        assert!(
            sq.contains(&format!("pubchannel_{n}:EL2004DefaultChannel{n}")),
            "missing outputs field channel_{n}:\n{src}"
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
        sq.contains("EL2004OpMode::Default(_)=>0usize"),
        "input_len Default arm should be 0:\n{src}"
    );
    assert!(
        sq.contains("EL2004OpMode::Default(_)=>1usize"),
        "output_len Default arm should be 1:\n{src}"
    );

    // Running bit offsets 0..3 across the four channel PDOs, written through the
    // match-bound `m.outputs` path.
    assert!(
        sq.contains("bits.set(0usize,m.outputs.channel_1.output)"),
        "missing channel_1 set@0:\n{src}"
    );
    assert!(
        sq.contains("bits.set(1usize,m.outputs.channel_2.output)"),
        "missing channel_2 set@1:\n{src}"
    );
    assert!(
        sq.contains("bits.set(2usize,m.outputs.channel_3.output)"),
        "missing channel_3 set@2:\n{src}"
    );
    assert!(
        sq.contains("bits.set(3usize,m.outputs.channel_4.output)"),
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

    // Two channel sub-structs under the Default variant.
    assert!(
        src.contains("pub struct EL3602DefaultAiInputsChannel1"),
        "missing channel 1 sub-struct:\n{src}"
    );
    assert!(
        src.contains("pub struct EL3602DefaultAiInputsChannel2"),
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
        sq.contains("m.inputs.ai_inputs_channel_1.underrange=bits[0usize]"),
        "channel 1 underrange should read bit 0:\n{src}"
    );
    assert!(
        sq.contains("m.inputs.ai_inputs_channel_2.underrange=bits[48usize]"),
        "channel 2 underrange should read bit 48 (after channel 1):\n{src}"
    );
    assert!(
        sq.contains("m.inputs.ai_inputs_channel_2.value=bits[64usize..96usize].load_le::<i32>()"),
        "channel 2 value should load 64..96:\n{src}"
    );

    // 96-bit total input guard (2 x 48), input_len 12 bytes, no outputs.
    assert!(
        sq.contains("constNEED:usize=96usize"),
        "missing 96-bit input guard:\n{src}"
    );
    assert!(
        sq.contains("EL3602OpMode::Default(_)=>12usize"),
        "input_len Default arm should be 12:\n{src}"
    );
    assert!(
        sq.contains("EL3602OpMode::Default(_)=>0usize"),
        "output_len Default arm should be 0:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// Sm-default folding (issue #70): a device with NO <AlternativeSmMapping>
// resolves to a SINGLE synthetic `Default` OpMode variant. PDOs carrying an
// `Sm=` attribute (or `Mandatory`) form the default set and are laid out
// side-by-side; `Fixed` is orthogonal to assignment.
// ---------------------------------------------------------------------------

/// The synthetic ALT device has two non-fixed, non-mandatory `TxPdos` sharing
/// Sm=3 ("Standard" 16-bit, "Compact" 8-bit) and no `<AlternativeSmMapping>`.
/// Under the joint-OpMode model both are in the Sm-default set, so they fold
/// into a single `Default` variant as two coexisting per-PDO sub-structs.
fn alt_source() -> String {
    let xml = include_str!("../../taktora-ethercat-esi/tests/fixtures/pdo_alternatives.xml");
    source_from_xml(xml)
}

#[test]
fn alt_emits_single_default_op_mode_variant() {
    let src = alt_source();
    let sq = squash(&src);

    // One OpMode enum with exactly the synthetic Default variant.
    assert!(
        src.contains("pub enum ALTOpMode"),
        "missing ALTOpMode enum:\n{src}"
    );
    assert!(
        sq.contains("Default(ALTDefault),"),
        "OpMode should have a single Default(ALTDefault) variant:\n{src}"
    );
    assert_eq!(
        sq.matches("Default(ALTDefault)").count(),
        1,
        "expected exactly one variant:\n{src}"
    );

    // Manual Default selects the first (only) variant.
    assert!(
        sq.contains("implDefaultforALTOpMode"),
        "missing manual Default impl:\n{src}"
    );
    assert!(
        sq.contains("Self::Default(Default::default())"),
        "Default should select the Default variant:\n{src}"
    );

    // The device struct carries the OpMode-typed `mode` field.
    assert!(
        sq.contains("pubmode:ALTOpMode"),
        "missing mode field on device:\n{src}"
    );
}

#[test]
fn alt_folds_both_pdos_into_default_inputs_as_substructs() {
    let src = alt_source();
    let sq = squash(&src);

    // Both PDOs coexist under the Default `inputs` direction struct as per-PDO
    // sub-structs (the multi-PDO split shape).
    assert!(
        src.contains("pub struct ALTDefaultStandard"),
        "missing Standard sub-struct:\n{src}"
    );
    assert!(
        src.contains("pub struct ALTDefaultCompact"),
        "missing Compact sub-struct:\n{src}"
    );
    assert!(
        sq.contains("pubstandard:ALTDefaultStandard")
            && sq.contains("pubcompact:ALTDefaultCompact"),
        "Default inputs should carry both PDO sub-struct fields:\n{src}"
    );

    // 16-bit untyped entry → u16; 8-bit untyped entry → u8.
    assert!(
        sq.contains(":u16"),
        "Standard PDO should hold a u16 entry:\n{src}"
    );
    assert!(
        sq.contains(":u8"),
        "Compact PDO should hold a u8 entry:\n{src}"
    );
}

#[test]
fn alt_decode_inputs_reads_both_pdos_at_running_offsets() {
    let src = alt_source();
    let sq = squash(&src);

    // decode_inputs matches the active mode; the single Default arm reads both
    // PDOs at running offsets (Standard 0..16, then Compact 16..24).
    assert!(
        sq.contains("match&mutself.mode"),
        "decode_inputs should match the active mode:\n{src}"
    );
    assert!(
        sq.contains("ALTOpMode::Default(m)=>"),
        "decode should have a Default arm:\n{src}"
    );
    assert!(
        sq.contains("m.inputs.standard.entry_6000_1=bits[0usize..16usize].load_le::<u16>()"),
        "Standard entry should load 0..16:\n{src}"
    );
    assert!(
        sq.contains("m.inputs.compact.entry_6000_1=bits[16usize..24usize].load_le::<u8>()"),
        "Compact entry should load at the running offset 16..24:\n{src}"
    );
}

#[test]
fn alt_pdo_assignment_lists_both_default_indices() {
    let src = alt_source();
    let sq = squash(&src);

    // pdo_assignment()'s Default arm lists both TxPdo indices (0x1A00=6656,
    // 0x1A01=6657) on tx, empty rx.
    assert!(
        sq.contains("ALTOpMode::Default(_)=>{PdoAssignment{rx:&[],tx:&[6656u16,6657u16],}}"),
        "pdo_assignment Default arm should list both tx indices:\n{src}"
    );
}

/// Regression: a direction with exactly one PDO keeps the FLAT shape — no
/// per-PDO sub-struct; the entry fields live directly on the variant's direction
/// struct (`<Dev>DefaultIn`), not under a `<Dev>Default<Pdo>` sub-struct.
#[test]
fn single_pdo_device_stays_flat() {
    let src = generated_source();
    // No per-PDO sub-struct type emitted (only the direction/variant structs).
    assert!(
        !src.contains("pub struct EL3001_likeDefaultTxPdo")
            && !src.contains("pub struct EL3001_likeDefaultAi")
            && !src.contains("pub struct EL3001_likeDefaultChannel"),
        "single-PDO direction must not emit a per-PDO sub-struct:\n{src}"
    );
    // Fields live directly on the Default `inputs` direction struct.
    let sq = squash(&src);
    assert!(
        sq.contains("pubstructEL3001_likeDefaultIn{pubunderrange:bool,pubvalue:i16,}"),
        "flat fields must remain on the direction struct:\n{src}"
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
// Sm-default coexistence (the EL1262 unblock): two non-fixed/non-mandatory PDOs
// on distinct sync managers both carry an `Sm=` attribute, so both join the
// single synthetic `Default` set and coexist as per-PDO sub-structs — no
// genuine per-direction alternatives, no choice between them.
// ---------------------------------------------------------------------------

/// The EL1262 shape: two non-fixed/non-mandatory `TxPdo`s on DISTINCT sync
/// managers (Sm 3 and Sm 4), no `<AlternativeSmMapping>`. They coexist (one per
/// channel). Both are in the Sm-default set, so they fold into the single
/// `Default` `OpMode` variant as per-PDO sub-structs (no per-direction choice).
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
fn distinct_sm_singletons_emit_single_default_variant() {
    let src = source_from_xml(EL1262_SHAPE_XML);
    let sq = squash(&src);

    // No genuine alternatives: a single synthetic Default OpMode variant.
    assert!(
        src.contains("pub enum EL1262LIKEOpMode"),
        "missing OpMode enum:\n{src}"
    );
    assert_eq!(
        sq.matches("Default(EL1262LIKEDefault)").count(),
        1,
        "coexisting Sm-default PDOs must fold into one Default variant:\n{src}"
    );
}

#[test]
fn distinct_sm_singletons_become_per_pdo_substructs() {
    let src = source_from_xml(EL1262_SHAPE_XML);
    // Two coexisting PDOs in one direction → per-PDO sub-structs under the
    // Default `inputs` direction struct, one per channel.
    assert!(
        src.contains("pub struct EL1262LIKEDefaultChannel1")
            && src.contains("pub struct EL1262LIKEDefaultChannel2"),
        "both PDOs should become per-PDO sub-structs:\n{src}"
    );
    let sq = squash(&src);
    assert!(
        sq.contains("pubchannel_1:EL1262LIKEDefaultChannel1")
            && sq.contains("pubchannel_2:EL1262LIKEDefaultChannel2"),
        "Default inputs should carry both sub-struct fields:\n{src}"
    );
}

#[test]
fn distinct_sm_singletons_thread_running_offset() {
    // Channel 1 occupies bits 0..16; Channel 2 reads at the running offset 16.
    let src = source_from_xml(EL1262_SHAPE_XML);
    let sq = squash(&src);
    assert!(
        sq.contains("m.inputs.channel_1.value=bits[0usize..16usize].load_le::<u16>()"),
        "channel 1 reads bits 0..16:\n{src}"
    );
    assert!(
        sq.contains("m.inputs.channel_2.value=bits[16usize..32usize].load_le::<u16>()"),
        "channel 2 reads at the running offset 16..32:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// Colliding AlternativeSmMapping names (dedup regression). Two mappings whose
// <Name>s PascalCase-collide ("Foo Bar" and "Foo-Bar" both → `FooBar`) must NOT
// produce two identical `<Dev>FooBar*` struct idents — that would emit
// `error[E0428]: defined multiple times`. The dedup must apply the SAME numeric
// suffix to BOTH the variant ident and the struct ident, in lockstep, BEFORE
// directions are resolved (so the per-PDO sub-structs of the multi-PDO mapping
// also inherit the suffix). The default mapping is multi-PDO on its output
// direction precisely to exercise that sub-struct cascade.
// ---------------------------------------------------------------------------

const COLLIDING_NAMES_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000002</Id></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="#x00030000" RevisionNo="#x00000001">COLLIDE</Type>
      <Name>COLLIDE</Name>
      <Info>
        <VendorSpecific>
          <TwinCAT>
            <AlternativeSmMapping Default="1">
              <Name>Foo Bar</Name>
              <Sm No="2"><Pdo ChannelNo="1">#x1600</Pdo><Pdo ChannelNo="2">#x1601</Pdo></Sm>
            </AlternativeSmMapping>
            <AlternativeSmMapping>
              <Name>Foo-Bar</Name>
              <Sm No="2"><Pdo ChannelNo="1">#x1602</Pdo></Sm>
            </AlternativeSmMapping>
          </TwinCAT>
        </VendorSpecific>
      </Info>
      <RxPdo Fixed="1" Sm="2"><Index>#x1600</Index><Name>Alpha</Name>
        <Entry><Index>#x7000</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>A</Name><DataType>UINT</DataType></Entry>
      </RxPdo>
      <RxPdo Fixed="1"><Index>#x1601</Index><Name>Beta</Name>
        <Entry><Index>#x7010</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>B</Name><DataType>UINT</DataType></Entry>
      </RxPdo>
      <RxPdo Fixed="1"><Index>#x1602</Index><Name>Gamma</Name>
        <Entry><Index>#x7020</Index><SubIndex>1</SubIndex><BitLen>16</BitLen><Name>G</Name><DataType>UINT</DataType></Entry>
      </RxPdo>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"##;

/// Collect every `pub struct <Ident>` / `pub enum <Ident>` name in the squashed
/// source. (Squashed so prettyplease formatting doesn't matter.)
fn pub_type_names(squashed: &str) -> Vec<String> {
    let mut names = Vec::new();
    for marker in ["pubstruct", "pubenum"] {
        let mut rest = squashed;
        while let Some(pos) = rest.find(marker) {
            let after = &rest[pos + marker.len()..];
            let ident: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                names.push(ident);
            }
            rest = after;
        }
    }
    names
}

#[test]
fn colliding_alt_sm_names_emit_no_duplicate_pub_types() {
    let src = source_from_xml(COLLIDING_NAMES_XML);
    let sq = squash(&src);

    // No `pub struct`/`pub enum` ident may be defined more than once. Plain
    // `syn::parse2` (in `source_from_xml`) accepts duplicate items, so this
    // explicit uniqueness check is what actually catches the E0428 bug.
    let names = pub_type_names(&sq);
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "duplicate pub struct/enum idents emitted (would not compile): {names:?}\n{src}"
    );

    // The two colliding mappings must yield two DISTINCT variants over two
    // DISTINCT struct idents: `FooBar(COLLIDEFooBar)` and
    // `FooBar2(COLLIDEFooBar2)`.
    assert!(
        sq.contains("FooBar(COLLIDEFooBar)"),
        "missing first (default) variant FooBar(COLLIDEFooBar):\n{src}"
    );
    assert!(
        sq.contains("FooBar2(COLLIDEFooBar2)"),
        "missing deduped second variant FooBar2(COLLIDEFooBar2):\n{src}"
    );

    // The default mapping is multi-PDO, so its per-PDO output sub-structs derive
    // from the (unsuffixed) struct ident `COLLIDEFooBar` — and must NOT clash
    // with the second mapping's `COLLIDEFooBar2` family.
    assert!(
        src.contains("pub struct COLLIDEFooBarOut"),
        "missing first variant output direction struct:\n{src}"
    );
    assert!(
        src.contains("pub struct COLLIDEFooBar2Out"),
        "missing deduped second variant output direction struct:\n{src}"
    );
}
