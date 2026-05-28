//! Working-counter-based health policy. `REQ_0319`, `REQ_0320`.
//!
//! Per cycle the gateway computes an expected WKC from the PDO
//! mapping and reads back the observed WKC from
//! `ethercrab::SubDeviceGroup::tx_rx`. This module provides the
//! decision: did the working counter match (→ `Up`) or come up short
//! (→ `Degraded` with a reason naming the offending cycle)?

/// Pure decision: did the working counter match expectation?
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WkcVerdict {
    /// `observed >= expected` — bus is healthy on this cycle.
    Match,
    /// `observed < expected` — at least one SubDevice didn't respond
    /// per the configured mapping. The fields are forwarded into the
    /// `ConnectorHealth::Degraded::reason` message.
    Mismatch {
        /// Working counter the SubDeviceGroup actually returned.
        observed: u16,
        /// Working counter the gateway expected from its mapping.
        expected: u16,
    },
}

impl WkcVerdict {
    /// Compose the human-readable reason string surfaced via
    /// [`taktora_connector_core::ConnectorHealth::Degraded`]. Only
    /// meaningful when `self` is [`WkcVerdict::Mismatch`].
    #[must_use]
    pub fn degraded_reason(&self, cycle_index: u64) -> Option<String> {
        match *self {
            Self::Match => None,
            Self::Mismatch { observed, expected } => Some(format!(
                "working counter below expected: cycle {cycle_index}, observed {observed}, expected {expected}"
            )),
        }
    }
}

/// Compare an observed WKC against the expected value. `observed >=
/// expected` is a [`WkcVerdict::Match`]; anything less is
/// [`WkcVerdict::Mismatch`] (`REQ_0319` / `REQ_0320`).
#[must_use]
pub const fn evaluate_wkc(expected: u16, observed: u16) -> WkcVerdict {
    if observed >= expected {
        WkcVerdict::Match
    } else {
        WkcVerdict::Mismatch { observed, expected }
    }
}

/// Sum of every [`crate::SubDeviceMap::expected_wkc`] in
/// `options.pdo_map()`.
///
/// The pure-logic helper used by [`crate::BusDriver::bring_up`] and
/// [`crate::BusDriver::recover`] to compute the WKC the cycle loop
/// compares each `tx_rx` response against.
///
/// `REQ_0329`. Computed without consulting the bus — every
/// SubDevice present on the bus but absent from `pdo_map`
/// contributes 0 by construction (we only iterate the map).
#[must_use]
pub fn expected_wkc_from_map(options: &crate::EthercatConnectorOptions) -> u16 {
    let mut total: u16 = 0;
    for map in options.pdo_map() {
        total = total.saturating_add(map.expected_wkc);
    }
    total
}
