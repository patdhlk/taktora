//! Pure 29-bit extended-CAN-id decode / encode for J1939. `BB_0099`,
//! `REQ_0890`.
//!
//! This is the load-bearing logic every later issue (#123 BAM, #124
//! RTS/CTS, #125 ETP, #126 address-claim) builds on, so it is kept as
//! a set of small, side-effect-free functions with exhaustive unit
//! tests. Nothing here touches iceoryx2, tokio, or the driver layer.
//!
//! ## 29-bit identifier layout (MSB → LSB)
//!
//! ```text
//!  bits 28..26  Priority (3 bits)
//!  bit  25      EDP  ─┐ together the two "data page" bits, captured as
//!  bit  24      DP   ─┘ `dp = (id >> 24) & 0x3`
//!  bits 23..16  PDU Format (PF)
//!  bits 15..8   PDU Specific (PS) — Destination Address (PDU1) or
//!               Group Extension (PDU2)
//!  bits 7..0    Source Address (SA)
//! ```
//!
//! * **PDU1 (destination-specific)** when `PF < 0xF0`: the PS field is
//!   the Destination Address (DA); the PGN's low byte is `0`, so
//!   `pgn = (dp << 16) | (pf << 8)`.
//! * **PDU2 (broadcast)** when `PF >= 0xF0`: the PS field is a Group
//!   Extension that is *part of* the PGN, so
//!   `pgn = (dp << 16) | (pf << 8) | ps`; there is no destination
//!   address (`da = None`).
//!
//! The PDU1-vs-PDU2 split is **derived from PF**, never declared by the
//! caller (`REQ_0890`).

use crate::routing::Pgn;

/// Fully decoded J1939 29-bit identifier.
///
/// Produced by [`decode_extended_id`]; consumed by
/// [`crate::routing::J1939Routing::matches`] during inbound demux.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedId {
    /// Priority — bits 28..26 (`0..=7`, lower is higher priority).
    pub priority: u8,
    /// PDU Format byte — bits 23..16. `< 0xF0` ⇒ PDU1, `>= 0xF0` ⇒
    /// PDU2.
    pub pdu_format: u8,
    /// Parameter Group Number (18-bit, validated).
    pub pgn: Pgn,
    /// Source Address — bits 7..0.
    pub source_addr: u8,
    /// Destination Address. `Some(addr)` for PDU1 (the PS field);
    /// `None` for PDU2 broadcast frames.
    pub dest_addr: Option<u8>,
}

impl DecodedId {
    /// `true` when this id is a destination-specific (PDU1) frame.
    #[must_use]
    pub const fn is_pdu1(&self) -> bool {
        self.pdu_format < PDU2_FORMAT_THRESHOLD
    }
}

/// PF values `>= 0xF0` are PDU2 (broadcast / group-extension) frames.
pub const PDU2_FORMAT_THRESHOLD: u8 = 0xF0;

/// Mask selecting the 29 valid bits of an extended identifier.
pub const EXTENDED_ID_MASK: u32 = 0x1FFF_FFFF;

/// Decode a raw 29-bit extended CAN identifier into its J1939 fields.
///
/// Bits above bit 28 are ignored (masked off), so callers may pass a
/// raw socketcan id with flag bits set without corrupting the decode.
///
/// The resulting [`DecodedId::pgn`] is always in range by construction
/// (PF and PS are byte fields, `dp` is 2 bits ⇒ at most 18 bits), so
/// the internal [`Pgn`] construction never fails.
#[must_use]
pub fn decode_extended_id(raw: u32) -> DecodedId {
    let id = raw & EXTENDED_ID_MASK;
    let priority = ((id >> 26) & 0x7) as u8;
    let dp = (id >> 24) & 0x3;
    let pf = ((id >> 16) & 0xFF) as u8;
    let ps = ((id >> 8) & 0xFF) as u8;
    let sa = (id & 0xFF) as u8;

    let (pgn_value, dest_addr) = if pf < PDU2_FORMAT_THRESHOLD {
        // PDU1: PS is the destination address; PGN low byte is zero.
        ((dp << 16) | (u32::from(pf) << 8), Some(ps))
    } else {
        // PDU2: PS is the group extension and part of the PGN.
        ((dp << 16) | (u32::from(pf) << 8) | u32::from(ps), None)
    };

    DecodedId {
        priority,
        pdu_format: pf,
        // SAFETY (logical): pgn_value fits 18 bits by construction.
        pgn: Pgn::new(pgn_value).unwrap_or(Pgn::ZERO),
        source_addr: sa,
        dest_addr,
    }
}

/// Reconstruct a 29-bit extended CAN identifier for transmission.
///
/// * `priority` is masked to 3 bits.
/// * The PF / DP bits come from `pgn`.
/// * For **PDU1** (`pgn`'s PF `< 0xF0`) the PS field carries the
///   destination address: `dest_addr` if supplied, else the J1939
///   global address `0xFF`.
/// * For **PDU2** (`pgn`'s PF `>= 0xF0`) the PS field is the PGN's own
///   low byte (group extension); `dest_addr` is ignored.
///
/// Round-trips with [`decode_extended_id`] for both PDU formats.
#[must_use]
pub fn encode_extended_id(pgn: Pgn, priority: u8, source_addr: u8, dest_addr: Option<u8>) -> u32 {
    let pgnv = pgn.value();
    let pf = (pgnv >> 8) & 0xFF;
    let dp = (pgnv >> 16) & 0x3;
    let ps = if (pf as u8) < PDU2_FORMAT_THRESHOLD {
        u32::from(dest_addr.unwrap_or(GLOBAL_ADDRESS))
    } else {
        pgnv & 0xFF
    };
    ((u32::from(priority) & 0x7) << 26)
        | (dp << 24)
        | (pf << 16)
        | (ps << 8)
        | u32::from(source_addr)
}

/// J1939 global (broadcast) destination address.
pub const GLOBAL_ADDRESS: u8 = 0xFF;

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST_0886 (unit): PDU1 destination-specific decode — Request
    /// PGN 59904 (0xEA00), priority 6, DA 0x21, SA 0x11.
    #[test]
    fn decode_pdu1_request_pgn_59904() {
        // (6<<26)|(0xEA<<16)|(0x21<<8)|0x11
        let raw = 0x18EA_2111;
        let d = decode_extended_id(raw);
        assert_eq!(d.priority, 6);
        assert_eq!(d.pdu_format, 0xEA);
        assert_eq!(d.pgn.value(), 59904);
        assert_eq!(d.source_addr, 0x11);
        assert_eq!(d.dest_addr, Some(0x21));
        assert!(d.is_pdu1());
    }

    /// PDU1 Address-Claimed PGN 60928 (0xEE00) — matters for #126.
    #[test]
    fn decode_pdu1_address_claimed_pgn_60928() {
        // priority 6, PF 0xEE, DA 0xFF (global), SA 0x80
        let raw = encode_extended_id(Pgn::new(60928).unwrap(), 6, 0x80, Some(0xFF));
        let d = decode_extended_id(raw);
        assert_eq!(d.pgn.value(), 60928);
        assert_eq!(d.pdu_format, 0xEE);
        assert_eq!(d.source_addr, 0x80);
        assert_eq!(d.dest_addr, Some(0xFF));
        assert!(d.is_pdu1());
    }

    /// TEST_0886 (unit): PDU2 broadcast decode — PF 0xFE, group
    /// extension 0xF6 ⇒ PGN 0xFEF6 (65270), no destination address.
    #[test]
    fn decode_pdu2_broadcast_pgn_65270() {
        // (3<<26)|(0xFE<<16)|(0xF6<<8)|0x80
        let raw = 0x0CFE_F680;
        let d = decode_extended_id(raw);
        assert_eq!(d.priority, 3);
        assert_eq!(d.pdu_format, 0xFE);
        assert_eq!(d.pgn.value(), 65270);
        assert_eq!(d.source_addr, 0x80);
        assert_eq!(d.dest_addr, None);
        assert!(!d.is_pdu1());
    }

    #[test]
    fn decode_ignores_flag_bits_above_29() {
        let raw = 0xFFFF_FFFF; // all flags set
        let d = decode_extended_id(raw);
        // PF 0xFF, PS 0xFF (PDU2), SA 0xFF, priority 7.
        assert_eq!(d.priority, 7);
        assert_eq!(d.source_addr, 0xFF);
        assert_eq!(d.dest_addr, None);
    }

    #[test]
    fn encode_decode_round_trip_pdu1() {
        let pgn = Pgn::new(59904).unwrap();
        let raw = encode_extended_id(pgn, 6, 0x11, Some(0x21));
        let d = decode_extended_id(raw);
        assert_eq!(d.pgn, pgn);
        assert_eq!(d.priority, 6);
        assert_eq!(d.source_addr, 0x11);
        assert_eq!(d.dest_addr, Some(0x21));
    }

    #[test]
    fn encode_decode_round_trip_pdu2() {
        let pgn = Pgn::new(65270).unwrap();
        let raw = encode_extended_id(pgn, 3, 0x80, None);
        let d = decode_extended_id(raw);
        assert_eq!(d.pgn, pgn);
        assert_eq!(d.priority, 3);
        assert_eq!(d.source_addr, 0x80);
        assert_eq!(d.dest_addr, None);
    }

    #[test]
    fn encode_pdu1_defaults_to_global_when_no_dest() {
        let pgn = Pgn::new(59904).unwrap();
        let raw = encode_extended_id(pgn, 6, 0x11, None);
        let d = decode_extended_id(raw);
        assert_eq!(d.dest_addr, Some(GLOBAL_ADDRESS));
    }
}
