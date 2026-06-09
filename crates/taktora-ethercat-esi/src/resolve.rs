//! Resolve a device's selected PDO assignment into PDO-granularity entries.
//!
//! Both `taktora-ethercat-netcfg` (bus map) and
//! `taktora-ethercat-esi-codegen-ethercrab` (device codec) call [`classify_assignment`]
//! so they never disagree on which PDOs belong to a mapping.

use crate::model::{AlternativeSmMapping, EsiDevice, Pdo};

/// One resolved PDO at PDO-mapping-object granularity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPdoEntry {
    /// PDO mapping-object index (e.g. `0x1601`), as written to `0x1C12`/`0x1C13`.
    pub index: u16,
    /// Cumulative bit offset within the direction's process image.
    pub bit_offset: u16,
    /// Total bits of the PDO (sum of its inner entries, incl. padding).
    pub bit_length: u16,
}

/// A resolved assignment: rx (outputs) + tx (inputs) PDO lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssignment {
    /// `RxPDOs` (master -> device, outputs).
    pub rx: Vec<ResolvedPdoEntry>,
    /// `TxPDOs` (device -> master, inputs).
    pub tx: Vec<ResolvedPdoEntry>,
}

/// Errors from [`EsiDevice::resolve_assignment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// `name` was given but the device declares no `AlternativeSmMapping`.
    NoAlternativeMappings,
    /// `name` did not match any mapping. `available` lists the names present.
    MappingNotFound {
        /// The requested mode name.
        requested: String,
        /// Named mappings the device does declare.
        available: Vec<String>,
    },
    /// `name` was omitted and no mapping carries `Default="1"`.
    NoDefaultMapping,
    /// A mapping references a PDO index found in neither `rx_pdos` nor `tx_pdos`.
    UnknownAssignmentPdo {
        /// The dangling PDO index.
        index: u16,
    },
}

/// Classify a mapping's assigned PDO indices into (rx, tx) by membership in the
/// device's `rx_pdos` / `tx_pdos`, in `sm_assignments` document order.
///
/// Shared with the ethercrab codegen backend so the bus map and the device codec
/// never disagree on a mapping's PDO membership/order.
pub fn classify_assignment(
    mapping: &AlternativeSmMapping,
    rx_pdos: &[Pdo],
    tx_pdos: &[Pdo],
) -> Result<(Vec<u16>, Vec<u16>), ResolveError> {
    let mut rx = Vec::new();
    let mut tx = Vec::new();
    for sm in &mapping.sm_assignments {
        for p in &sm.pdos {
            if rx_pdos.iter().any(|q| q.index == p.index) {
                rx.push(p.index);
            } else if tx_pdos.iter().any(|q| q.index == p.index) {
                tx.push(p.index);
            } else {
                return Err(ResolveError::UnknownAssignmentPdo { index: p.index });
            }
        }
    }
    Ok((rx, tx))
}

fn pdo_total_bits(pool: &[Pdo], index: u16) -> u16 {
    pool.iter().find(|p| p.index == index).map_or(0, |p| {
        p.entries
            .iter()
            .fold(0u16, |acc, e| acc.saturating_add(e.bit_length))
    })
}

fn entries_for(pool: &[Pdo], indices: &[u16]) -> Vec<ResolvedPdoEntry> {
    let mut out = Vec::with_capacity(indices.len());
    let mut bit_offset: u16 = 0;
    for &index in indices {
        let bit_length = pdo_total_bits(pool, index);
        out.push(ResolvedPdoEntry {
            index,
            bit_offset,
            bit_length,
        });
        bit_offset = bit_offset.saturating_add(bit_length);
    }
    out
}

impl EsiDevice {
    /// Resolve the selected PDO assignment into PDO-granularity rx/tx entries.
    ///
    /// `name == Some(n)` selects the `AlternativeSmMapping` whose `name == Some(n)`.
    /// `name == None` selects the `Default="1"` mapping; if the device declares no
    /// mappings at all, the synthetic default set is every PDO with an `Sm=`
    /// attribute or `Mandatory` (matching the codegen's default assignment).
    ///
    /// # Errors
    /// See [`ResolveError`].
    pub fn resolve_assignment(
        &self,
        name: Option<&str>,
    ) -> Result<ResolvedAssignment, ResolveError> {
        if self.alt_sm_mappings.is_empty() {
            if name.is_some() {
                return Err(ResolveError::NoAlternativeMappings);
            }
            let rx_idx: Vec<u16> = self
                .rx_pdos
                .iter()
                .filter(|p| p.sm.is_some() || p.mandatory)
                .map(|p| p.index)
                .collect();
            let tx_idx: Vec<u16> = self
                .tx_pdos
                .iter()
                .filter(|p| p.sm.is_some() || p.mandatory)
                .map(|p| p.index)
                .collect();
            return Ok(ResolvedAssignment {
                rx: entries_for(&self.rx_pdos, &rx_idx),
                tx: entries_for(&self.tx_pdos, &tx_idx),
            });
        }

        let mapping = match name {
            Some(n) => self
                .alt_sm_mappings
                .iter()
                .find(|m| m.name.as_deref() == Some(n))
                .ok_or_else(|| ResolveError::MappingNotFound {
                    requested: n.into(),
                    available: self
                        .alt_sm_mappings
                        .iter()
                        .filter_map(|m| m.name.clone())
                        .collect(),
                })?,
            None => self
                .alt_sm_mappings
                .iter()
                .find(|m| m.default)
                .ok_or(ResolveError::NoDefaultMapping)?,
        };

        let (rx_idx, tx_idx) = classify_assignment(mapping, &self.rx_pdos, &self.tx_pdos)?;
        Ok(ResolvedAssignment {
            rx: entries_for(&self.rx_pdos, &rx_idx),
            tx: entries_for(&self.tx_pdos, &tx_idx),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AltPdoRef, PdoEntry, SmAssignment};

    fn pdo(index: u16, entry_bits: &[u16]) -> Pdo {
        Pdo {
            index,
            name: None,
            sm: Some(3),
            fixed: false,
            mandatory: false,
            exclude: Vec::new(),
            entries: entry_bits
                .iter()
                .map(|&b| PdoEntry {
                    index: 0x6000,
                    sub_index: 0,
                    bit_length: b,
                    name: None,
                    data_type: None,
                })
                .collect(),
        }
    }

    fn mapping(name: &str, default: bool, rx: &[u16], tx: &[u16]) -> AlternativeSmMapping {
        let mut sm_assignments = Vec::new();
        if !rx.is_empty() {
            sm_assignments.push(SmAssignment {
                sm: 2,
                pdos: rx
                    .iter()
                    .map(|&i| AltPdoRef {
                        index: i,
                        channel_no: None,
                    })
                    .collect(),
            });
        }
        if !tx.is_empty() {
            sm_assignments.push(SmAssignment {
                sm: 3,
                pdos: tx
                    .iter()
                    .map(|&i| AltPdoRef {
                        index: i,
                        channel_no: None,
                    })
                    .collect(),
            });
        }
        AlternativeSmMapping {
            name: Some(name.into()),
            default,
            sm_assignments,
        }
    }

    fn el7047_like() -> EsiDevice {
        EsiDevice {
            identity: crate::Identity {
                vendor_id: 2,
                product_code: 1,
                revision: 1,
            },
            name: None,
            product_type: None,
            group_type: None,
            fmmus: Vec::new(),
            sync_managers: Vec::new(),
            rx_pdos: vec![
                pdo(0x1600, &[8, 8]),
                pdo(0x1601, &[8, 32, 16, 16, 16, 16, 16]),
            ],
            tx_pdos: vec![pdo(0x1a00, &[8, 8]), pdo(0x1a01, &[8, 32, 32, 16, 32])],
            mailbox: None,
            dc: None,
            dictionary: Vec::new(),
            eeprom: None,
            slots: None,
            alt_sm_mappings: vec![
                mapping("Velocity control compact", true, &[0x1600], &[0x1a00]),
                mapping("Positioning interface", false, &[0x1601], &[0x1a01]),
            ],
            vendor_extensions: Vec::new(),
        }
    }

    #[test]
    fn named_mapping_resolves_to_pdo_indices_and_summed_lengths() {
        let dev = el7047_like();
        let a = dev
            .resolve_assignment(Some("Positioning interface"))
            .expect("resolves");
        assert_eq!(
            a.rx,
            vec![ResolvedPdoEntry {
                index: 0x1601,
                bit_offset: 0,
                bit_length: 8 + 32 + 16 + 16 + 16 + 16 + 16
            }]
        );
        assert_eq!(
            a.tx,
            vec![ResolvedPdoEntry {
                index: 0x1a01,
                bit_offset: 0,
                bit_length: 8 + 32 + 32 + 16 + 32
            }]
        );
    }

    #[test]
    fn omitted_name_uses_default_mapping() {
        let dev = el7047_like();
        let a = dev.resolve_assignment(None).expect("default resolves");
        assert_eq!(a.rx[0].index, 0x1600);
        assert_eq!(a.tx[0].index, 0x1a00);
    }

    #[test]
    fn unknown_name_lists_available() {
        let dev = el7047_like();
        let err = dev.resolve_assignment(Some("nope")).unwrap_err();
        assert_eq!(
            err,
            ResolveError::MappingNotFound {
                requested: "nope".into(),
                available: vec![
                    "Velocity control compact".into(),
                    "Positioning interface".into(),
                ],
            }
        );
    }

    #[test]
    fn name_on_device_without_mappings_errors() {
        let mut dev = el7047_like();
        dev.alt_sm_mappings.clear();
        assert_eq!(
            dev.resolve_assignment(Some("x")).unwrap_err(),
            ResolveError::NoAlternativeMappings
        );
    }

    #[test]
    fn no_mappings_default_set_uses_sm_or_mandatory() {
        let mut dev = el7047_like();
        dev.alt_sm_mappings.clear();
        let a = dev.resolve_assignment(None).expect("synthetic default");
        assert_eq!(
            a.rx.iter().map(|e| e.index).collect::<Vec<_>>(),
            vec![0x1600, 0x1601]
        );
        assert_eq!(
            a.tx.iter().map(|e| e.index).collect::<Vec<_>>(),
            vec![0x1a00, 0x1a01]
        );
        // Cumulative offset: 0x1600 totals 8+8=16 bits, so 0x1601 starts at 16.
        assert_eq!(a.rx[0].bit_offset, 0);
        assert_eq!(a.rx[1].bit_offset, 16);
        assert_eq!(a.tx[1].bit_offset, a.tx[0].bit_length);
    }

    #[test]
    fn no_default_among_mappings_errors() {
        let mut dev = el7047_like();
        for m in &mut dev.alt_sm_mappings {
            m.default = false;
        }
        assert_eq!(
            dev.resolve_assignment(None).unwrap_err(),
            ResolveError::NoDefaultMapping
        );
    }
}
