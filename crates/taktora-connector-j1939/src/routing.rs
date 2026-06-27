//! Typed routing for the J1939 connector. `BB_0099`, `REQ_0890`,
//! `REQ_0891`.
//!
//! [`J1939Routing`] identifies one logical channel by its Parameter
//! Group Number plus optional source-/destination-address filters, the
//! transport class (single-frame vs multi-packet TP), and the TX
//! priority. PDU1-vs-PDU2 is *derived* from the PGN's PF field
//! ([`crate::decode`]), never declared by the caller (`REQ_0890`).

use taktora_connector_core::Routing;

use crate::decode::DecodedId;

/// Largest valid Parameter Group Number — 18 bits (EDP + DP + PF + PS).
pub const PGN_MAX: u32 = 0x3FFFF;

/// J1939 maximum transport-protocol payload (ETP upper bound), in
/// bytes. A [`TransportClass::Tp`] `max_len` is capped at this value.
pub const TP_MAX_LEN: usize = 1785;

/// Single classical-CAN frame payload size.
pub const SINGLE_FRAME_LEN: usize = 8;

/// Validated 18-bit Parameter Group Number newtype.
///
/// Construction enforces the 18-bit [`PGN_MAX`] range so downstream
/// decode / encode (`crate::decode`) can treat the value as a trusted
/// 18-bit quantity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct Pgn(u32);

/// Failure modes of [`Pgn::new`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PgnError {
    /// Value exceeds the 18-bit [`PGN_MAX`] range.
    #[error("PGN exceeds 18 bits (max {PGN_MAX:#x})")]
    Overflow,
}

impl Pgn {
    /// The all-zero PGN. Used as the decode fallback (never produced
    /// from a real frame because PF=0 is itself a valid PDU1 PGN, but a
    /// safe in-range default).
    pub const ZERO: Self = Self(0);

    /// Construct a validated PGN.
    ///
    /// # Errors
    ///
    /// Returns [`PgnError::Overflow`] when `value` exceeds
    /// [`PGN_MAX`].
    pub const fn new(value: u32) -> Result<Self, PgnError> {
        if value > PGN_MAX {
            return Err(PgnError::Overflow);
        }
        Ok(Self(value))
    }

    /// Raw 18-bit value.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }

    /// PDU Format byte of this PGN (bits 15..8 of the PGN).
    #[must_use]
    pub const fn pdu_format(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// `true` when this PGN is destination-specific (PDU1, PF `< 0xF0`).
    #[must_use]
    pub const fn is_pdu1(self) -> bool {
        self.pdu_format() < crate::decode::PDU2_FORMAT_THRESHOLD
    }
}

/// How a channel's payloads cross the wire.
///
/// * [`TransportClass::SingleFrame`] — a single classical-CAN frame,
///   `<= 8` data bytes. Fully implemented in this tracer bullet.
/// * [`TransportClass::Tp`] — multi-packet J1939 Transport Protocol,
///   `<= 1785` bytes. Channel sizing and `N` validation work today, but
///   multi-packet *send/recv* is delivered by issue #123 (BAM /
///   RTS-CTS) and #125 (ETP). See [`crate::dispatcher`] for the seam.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TransportClass {
    /// Single classical frame, `<= 8` bytes.
    SingleFrame,
    /// Multi-packet transport (BAM / RTS-CTS), `<= 1785` bytes. Rides a
    /// fixed-`N` [`taktora_connector_transport_iox::ConnectorEnvelope`]
    /// typed channel (`ADR_0109` tier 1).
    Tp {
        /// Maximum reassembled payload length. Capped at [`TP_MAX_LEN`]
        /// by [`TransportClass::max_payload`].
        max_len: usize,
    },
    /// J1939 ETP (Extended Transport Protocol), `1786..` bytes (#125,
    /// `REQ_0894`). ETP payloads are variable-length and cannot carry a
    /// compile-time `N`, so they ride the FEAT_0097 large-payload **slice
    /// channel** (`ADR_0109` tier 2), NOT the const-`N`
    /// `create_writer`/`create_reader` typed path. The reassembly buffer
    /// is bounded by the connector's `max_etp_bytes`; a session announcing
    /// a larger total is aborted (`REQ_0903`). `max_len` is the per-channel
    /// ceiling, further clamped to `max_etp_bytes` at the connector.
    Etp {
        /// Maximum reassembled payload length for this channel (bytes).
        max_len: usize,
    },
}

impl TransportClass {
    /// Maximum payload bytes for this class — the value the channel's
    /// `N` const generic must equal (`REQ_0891`).
    ///
    /// * `SingleFrame` → 8
    /// * `Tp { max_len }` → `max_len`, capped at [`TP_MAX_LEN`]
    #[must_use]
    pub const fn max_payload(self) -> usize {
        match self {
            Self::SingleFrame => SINGLE_FRAME_LEN,
            Self::Tp { max_len } => {
                if max_len > TP_MAX_LEN {
                    TP_MAX_LEN
                } else {
                    max_len
                }
            }
            // ETP rides the slice channel, not a fixed-`N` envelope, so
            // its "max payload" is the per-channel ceiling verbatim (the
            // connector clamps it to `max_etp_bytes`). It is never used for
            // `N` validation.
            Self::Etp { max_len } => max_len,
        }
    }

    /// `true` for the BAM/RTS-CTS multi-packet transport class (rides a
    /// fixed-`N` envelope channel). ETP is NOT included — see
    /// [`Self::is_etp`].
    #[must_use]
    pub const fn is_tp(self) -> bool {
        matches!(self, Self::Tp { .. })
    }

    /// `true` for the ETP transport class (rides the FEAT_0097 slice
    /// channel, #125).
    #[must_use]
    pub const fn is_etp(self) -> bool {
        matches!(self, Self::Etp { .. })
    }
}

/// Default J1939 priority for general broadcast traffic (3 is the
/// usual proprietary/broadcast default; control traffic uses lower).
pub const DEFAULT_PRIORITY: u8 = 6;

/// Identifies one logical channel by PGN + optional address filters +
/// transport class + TX priority.
///
/// Implements [`Routing`] (`REQ_0224`): `Clone + Send + Sync + Debug +
/// 'static`, no methods of its own.
///
/// Address filters use `None` as a wildcard (`REQ_0890`):
///
/// * `source_addr = None` — accept any source on inbound; on outbound
///   the dispatcher substitutes the interface's configured source
///   address.
/// * `dest_addr = None` — accept any destination on inbound (and, for
///   PDU2 broadcast frames which have no DA, this is the only value
///   that matches).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct J1939Routing {
    /// Parameter Group Number this channel carries.
    pub pgn: Pgn,
    /// Optional source-address filter (`None` = wildcard).
    pub source_addr: Option<u8>,
    /// Optional destination-address filter (`None` = wildcard).
    pub dest_addr: Option<u8>,
    /// Single-frame or multi-packet transport; sets the channel's
    /// payload sizing per `REQ_0891`.
    pub transport: TransportClass,
    /// TX priority (0..=7) used when encoding outbound identifiers.
    pub priority: u8,
}

impl J1939Routing {
    /// Construct a single-frame routing with default priority and no
    /// address filters (wildcard on both).
    #[must_use]
    pub const fn single_frame(pgn: Pgn) -> Self {
        Self {
            pgn,
            source_addr: None,
            dest_addr: None,
            transport: TransportClass::SingleFrame,
            priority: DEFAULT_PRIORITY,
        }
    }

    /// Construct a multi-packet (TP) routing with default priority and
    /// no address filters.
    #[must_use]
    pub const fn tp(pgn: Pgn, max_len: usize) -> Self {
        Self {
            pgn,
            source_addr: None,
            dest_addr: None,
            transport: TransportClass::Tp { max_len },
            priority: DEFAULT_PRIORITY,
        }
    }

    /// Construct an ETP routing (#125) with default priority and no
    /// address filters. `max_len` is the per-channel reassembly ceiling
    /// (clamped to `max_etp_bytes` at the connector). ETP rides the
    /// FEAT_0097 slice channel via `create_etp_writer` /
    /// `create_etp_reader`.
    #[must_use]
    pub const fn etp(pgn: Pgn, max_len: usize) -> Self {
        Self {
            pgn,
            source_addr: None,
            dest_addr: None,
            transport: TransportClass::Etp { max_len },
            priority: DEFAULT_PRIORITY,
        }
    }

    /// Builder-style source-address filter.
    #[must_use]
    pub const fn with_source_addr(mut self, sa: u8) -> Self {
        self.source_addr = Some(sa);
        self
    }

    /// Builder-style destination-address filter.
    #[must_use]
    pub const fn with_dest_addr(mut self, da: u8) -> Self {
        self.dest_addr = Some(da);
        self
    }

    /// Builder-style TX priority override.
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Demux predicate (`REQ_0890`): a decoded inbound id matches this
    /// routing when the PGN is equal AND each present address filter
    /// matches (`None` filters are wildcards).
    ///
    /// A `dest_addr` filter of `Some(_)` never matches a PDU2 broadcast
    /// frame (whose `decoded.dest_addr` is `None`) — broadcast frames
    /// carry no destination address.
    #[must_use]
    pub fn matches(&self, decoded: &DecodedId) -> bool {
        self.pgn == decoded.pgn
            && self.source_addr.is_none_or(|sa| sa == decoded.source_addr)
            && self
                .dest_addr
                .is_none_or(|da| decoded.dest_addr == Some(da))
    }
}

impl Routing for J1939Routing {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_extended_id;

    #[test]
    fn pgn_caps_at_18_bits() {
        assert!(Pgn::new(PGN_MAX).is_ok());
        assert_eq!(Pgn::new(PGN_MAX + 1), Err(PgnError::Overflow));
    }

    #[test]
    fn pgn_pdu_classification() {
        assert!(Pgn::new(59904).unwrap().is_pdu1()); // PF 0xEA
        assert!(!Pgn::new(65270).unwrap().is_pdu1()); // PF 0xFE
    }

    #[test]
    fn transport_class_max_payload() {
        assert_eq!(TransportClass::SingleFrame.max_payload(), 8);
        assert_eq!(TransportClass::Tp { max_len: 100 }.max_payload(), 100);
        assert_eq!(
            TransportClass::Tp { max_len: 9000 }.max_payload(),
            TP_MAX_LEN
        );
    }

    #[test]
    fn wildcard_matches_any_source_and_dest() {
        let r = J1939Routing::single_frame(Pgn::new(59904).unwrap());
        // PDU1 frame, SA 0x11, DA 0x21.
        let d = decode_extended_id(0x18EA_2111);
        assert!(r.matches(&d));
    }

    #[test]
    fn source_filter_rejects_other_sources() {
        let r = J1939Routing::single_frame(Pgn::new(59904).unwrap()).with_source_addr(0x22);
        let d = decode_extended_id(0x18EA_2111); // SA 0x11
        assert!(!r.matches(&d));
    }

    #[test]
    fn dest_filter_never_matches_broadcast() {
        let r = J1939Routing::single_frame(Pgn::new(65270).unwrap()).with_dest_addr(0x21);
        let d = decode_extended_id(0x0CFE_F680); // PDU2, dest None
        assert!(!r.matches(&d));
    }

    #[test]
    fn pgn_mismatch_does_not_match() {
        let r = J1939Routing::single_frame(Pgn::new(60928).unwrap());
        let d = decode_extended_id(0x18EA_2111); // PGN 59904
        assert!(!r.matches(&d));
    }
}
