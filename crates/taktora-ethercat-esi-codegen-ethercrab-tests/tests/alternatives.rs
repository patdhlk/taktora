//! Compile + decode gate for the selectable-PDO fixture (`esi/pdo_alternatives.xml`).
//!
//! After the joint-OpMode backend rewrite (issue #70), `pdo_alternatives.xml` —
//! which carries NO `<AlternativeSmMapping>` — no longer resolves to a
//! per-direction choice enum. Its two non-fixed, non-mandatory `TxPdo`s on Sm 3
//! (`Standard`, 16-bit, and `Compact`, 8-bit) instead fold into the single
//! `Default` OpMode image as two sub-structs (`standard`, `compact`) that
//! coexist back-to-back in the PDI. By living in `esi/*.xml` this code is
//! actually COMPILED via the crate's `generated` module and decode-tested here.
//!
//! We assert the new single-`Default`-variant shape: the OpMode enum, the
//! folded sub-structs, and that each entry decodes from its own bit offset
//! (`standard` at bits 0..16, `compact` at bits 16..24).

use bitvec::view::BitView;
use taktora_ethercat_esi_codegen_ethercrab_tests::generated::{
    ALT, ALTDefault, ALTDefaultCompact, ALTDefaultIn, ALTDefaultStandard, ALTOpMode,
};
use taktora_ethercat_esi_rt::{BitSlice, EsiDevice, Lsb0};

/// With no `<AlternativeSmMapping>`, the fixture resolves to a single `Default`
/// OpMode variant (selected by the generated `Default` impl).
#[test]
fn alt_default_is_single_default_variant() {
    let dev = ALT::default();
    assert!(
        matches!(dev.mode, ALTOpMode::Default(_)),
        "fixture without AlternativeSmMapping should resolve to a single Default mode"
    );
}

/// Both selectable PDOs fold into the `Default` image: `standard` (16-bit) at
/// bits 0..16 and `compact` (8-bit) at bits 16..24, decoded together.
#[test]
fn alt_default_decodes_both_folded_pdos() {
    let mut dev = ALT::default();

    // 3-byte PDI: standard = 0x1234 (bits 0..16, LE) -> [0x34, 0x12];
    //             compact  = 0xAB    (bits 16..24)    -> [0xAB].
    let bytes: [u8; 3] = [0x34, 0x12, 0xAB];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    dev.decode_inputs(bits)
        .expect("ALT Default decode should succeed");

    let ALTOpMode::Default(ref m) = dev.mode;
    assert_eq!(
        m.inputs.standard.entry_6000_1, 0x1234,
        "Standard entry should decode from bits 0..16"
    );
    assert_eq!(
        m.inputs.compact.entry_6000_1, 0xAB,
        "Compact entry should decode from bits 16..24"
    );

    assert_eq!(
        dev.input_len(),
        3,
        "ALT folds both PDOs: 2 bytes (standard) + 1 byte (compact)"
    );
    assert_eq!(dev.output_len(), 0, "ALT has no outputs");
}

/// The folded sub-structs are constructible and round-trip through the single
/// `Default` variant (smoke that both leaf field types — `u16`, `u8` — compile).
#[test]
fn alt_default_struct_is_constructible() {
    let dev = ALT {
        mode: ALTOpMode::Default(ALTDefault {
            inputs: ALTDefaultIn {
                standard: ALTDefaultStandard {
                    entry_6000_1: 0xBEEF,
                },
                compact: ALTDefaultCompact { entry_6000_1: 0x42 },
            },
            ..Default::default()
        }),
    };
    let ALTOpMode::Default(m) = dev.mode;
    assert_eq!(m.inputs.standard.entry_6000_1, 0xBEEF);
    assert_eq!(m.inputs.compact.entry_6000_1, 0x42);
}
