//! Compile + decode gate for the PDO-assignment alternatives path (T9,
//! REQ_0523/0524).
//!
//! The `ALT` synthetic fixture (`esi/pdo_alternatives.xml`) has two non-fixed,
//! non-mandatory `TxPdo`s on Sm 3 — `Standard` (16-bit) and `Compact` (8-bit) —
//! which codegen resolves into a single alternative group: a `ALTPdoAssignment`
//! choice enum with one tuple variant per alternative. By being part of
//! `esi/*.xml` this code is actually COMPILED (via the crate's `generated`
//! module) — the gap the review flagged was that the alternatives path had only
//! ever been `syn`-parse validated, never compiled or decode-tested.
//!
//! Here we construct each active variant, feed a PDI `BitSlice`, and assert the
//! active variant's entry decodes correctly.

use bitvec::view::BitView;
use taktora_ethercat_esi_codegen_ethercrab_tests::generated::{
    ALT, ALTPdoAssignment, ALTPdoCompact, ALTPdoStandard,
};
use taktora_ethercat_esi_rt::{BitSlice, EsiDevice, Lsb0};

/// The default alternative is the first declared one (`Standard`); the manual
/// `Default` impl selects it because the enum's variants are non-unit.
#[test]
fn alt_default_is_standard() {
    let dev = ALT::default();
    assert!(
        matches!(dev.pdo, ALTPdoAssignment::Standard(_)),
        "default alternative should be the first declared (Standard)"
    );
}

/// With the default `Standard` (16-bit) variant active, decode reads the entry
/// from bits 0..16 little-endian.
#[test]
fn alt_standard_variant_decodes_16_bits() {
    let mut dev = ALT::default();
    assert!(matches!(dev.pdo, ALTPdoAssignment::Standard(_)));

    // 2-byte PDI: 0x1234 little-endian in bits 0..16 -> [0x34, 0x12].
    let bytes: [u8; 2] = [0x34, 0x12];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    dev.decode_inputs(bits)
        .expect("ALT Standard decode should succeed");

    match &dev.pdo {
        ALTPdoAssignment::Standard(v) => {
            assert_eq!(
                v.entry_6000_1, 0x1234,
                "Standard entry should decode to 0x1234"
            );
        }
        ALTPdoAssignment::Compact(_) => panic!("expected the Standard variant to stay active"),
    }

    assert_eq!(
        dev.input_len(),
        2,
        "ALT advertises the widest (Standard) layout: 2 bytes"
    );
    assert_eq!(dev.output_len(), 0, "ALT has no outputs");
}

/// With the `Compact` (8-bit) variant active, decode reads the entry from bits
/// 0..8 — proving the alternate arm of the generated `match` compiles and
/// decodes against the same base offset.
#[test]
fn alt_compact_variant_decodes_8_bits() {
    let mut dev = ALT {
        pdo: ALTPdoAssignment::Compact(ALTPdoCompact::default()),
    };

    let bytes: [u8; 1] = [0xAB];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    dev.decode_inputs(bits)
        .expect("ALT Compact decode should succeed");

    match &dev.pdo {
        ALTPdoAssignment::Compact(v) => {
            assert_eq!(v.entry_6000_1, 0xAB, "Compact entry should decode to 0xAB");
        }
        ALTPdoAssignment::Standard(_) => panic!("expected the Compact variant to stay active"),
    }
}

/// The per-variant structs are constructible and round-trip through the enum
/// (a tiny smoke that the `Standard` struct compiles with its 16-bit field).
#[test]
fn alt_standard_struct_is_constructible() {
    let dev = ALT {
        pdo: ALTPdoAssignment::Standard(ALTPdoStandard {
            entry_6000_1: 0xBEEF,
        }),
    };
    match dev.pdo {
        ALTPdoAssignment::Standard(v) => assert_eq!(v.entry_6000_1, 0xBEEF),
        ALTPdoAssignment::Compact(_) => panic!("expected Standard"),
    }
}
