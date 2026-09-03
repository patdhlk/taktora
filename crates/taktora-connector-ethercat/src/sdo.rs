//! SDO write sequence for applying a [`SubDeviceMap`]'s PDO assignment
//! during the PRE-OP → SAFE-OP transition. `REQ_0315`.
//!
//! The sequence for one direction (RxPDO via `0x1C12`, TxPDO via
//! `0x1C13`) is fixed by CoE:
//!
//! 1. Write `0u8` to subindex 0 — clears the current count.
//! 2. For each entry, write the entry's `index` (`u16`) to subindex
//!    `1, 2, 3, …`.
//! 3. Write the entry count back to subindex 0 (`u8`).
//!
//! This module emits an iterator of [`SdoWrite`] values describing
//! exactly that sequence. Sequencing is pure-logic so it can be
//! unit-tested without `ethercrab` or a real bus.

use crate::options::{PdoEntry, SubDeviceMap};

/// Sync-manager PDO assignment index for the RxPDO direction.
pub const SM_ASSIGN_RX_PDO: u16 = 0x1C12;
/// Sync-manager PDO assignment index for the TxPDO direction.
pub const SM_ASSIGN_TX_PDO: u16 = 0x1C13;

/// One SDO write — index / subindex / value triple plus target.
///
/// Carries the SubDevice configured address; the (ethercrab-backed)
/// gateway translates each [`SdoWrite`] into one
/// `subdevice.sdo_write(...)` call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdoWrite {
    /// SubDevice configured address on the EtherCAT bus.
    pub subdevice_address: u16,
    /// SDO object dictionary index.
    pub index: u16,
    /// SDO object dictionary subindex.
    pub subindex: u8,
    /// Value to write. PDO-assignment writes use only `U8` (count) and
    /// `U16` (entry-index); startup-SDO writes (`REQ_0853`) may use any
    /// [`SdoValue`] variant.
    pub value: SdoValue,
}

/// SDO write value.
///
/// `U8`/`U16` cover the PDO-assignment sequence (`REQ_0315`); the wider and
/// signed variants exist for operator-declared startup-configuration SDOs
/// (`REQ_0853`), where drive parameters are commonly 16/32-bit and
/// occasionally signed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdoValue {
    /// 8-bit unsigned.
    U8(u8),
    /// 16-bit unsigned.
    U16(u16),
    /// 32-bit unsigned.
    U32(u32),
    /// 8-bit signed.
    I8(i8),
    /// 16-bit signed.
    I16(i16),
    /// 32-bit signed.
    I32(i32),
}

/// Emit the full SDO write sequence for one [`SubDeviceMap`].
///
/// Empty RxPDO / TxPDO lists produce no writes for that direction —
/// not even a clear-count, since the SubDevice's default mapping is
/// the desired state when the application has nothing to assign.
#[must_use]
pub fn pdo_sdo_writes(map: &SubDeviceMap) -> Vec<SdoWrite> {
    let mut out = Vec::new();
    push_direction(&mut out, map.address, SM_ASSIGN_RX_PDO, map.rx_pdos);
    push_direction(&mut out, map.address, SM_ASSIGN_TX_PDO, map.tx_pdos);
    out
}

/// Emit the operator-declared startup-configuration SDO writes for one
/// [`SubDeviceMap`]. `REQ_0853`.
///
/// Each entry becomes one [`SdoWrite`] in declaration order, stamped
/// with the map's address. Applied during PRE-OP **before** the
/// PDO-assignment writes from [`pdo_sdo_writes`] so config (e.g. motor
/// current) is in place before the mapping is committed.
#[must_use]
pub fn startup_sdo_writes(map: &SubDeviceMap) -> Vec<SdoWrite> {
    map.startup_sdos
        .iter()
        .map(|s| SdoWrite {
            subdevice_address: map.address,
            index: s.index,
            subindex: s.subindex,
            value: s.value,
        })
        .collect()
}

fn push_direction(out: &mut Vec<SdoWrite>, address: u16, sm_index: u16, entries: &[PdoEntry]) {
    if entries.is_empty() {
        return;
    }

    // Step 1 — clear count.
    out.push(SdoWrite {
        subdevice_address: address,
        index: sm_index,
        subindex: 0,
        value: SdoValue::U8(0),
    });

    // Step 2 — assign each entry. Subindex starts at 1.
    for (i, entry) in entries.iter().enumerate() {
        let subindex = u8::try_from(i + 1)
            .expect("PDO entries fit in u8 subindex; SubDeviceMap is a static slice ≤ 255");
        out.push(SdoWrite {
            subdevice_address: address,
            index: sm_index,
            subindex,
            value: SdoValue::U16(entry.index),
        });
    }

    // Step 3 — set count.
    let count = u8::try_from(entries.len())
        .expect("PDO entries fit in u8 count; SubDeviceMap is a static slice ≤ 255");
    out.push(SdoWrite {
        subdevice_address: address,
        index: sm_index,
        subindex: 0,
        value: SdoValue::U8(count),
    });
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use crate::options::{StartupSdo, SubDeviceMap};

    /// `TEST_0869` — one SDO write per `StartupSdo`, addressed to the map's
    /// `SubDevice`, in declaration order (`REQ_0853`).
    #[test]
    fn startup_writes_carry_map_address_and_order() {
        static STARTUP: &[StartupSdo] = &[
            StartupSdo {
                index: 0x8010,
                subindex: 0x01,
                value: SdoValue::U16(1800),
            },
            StartupSdo {
                index: 0x8010,
                subindex: 0x02,
                value: SdoValue::U16(900),
            },
        ];
        let map = SubDeviceMap::new(0x1003, &[], &[], 3).with_startup_sdos(STARTUP);
        let writes = startup_sdo_writes(&map);
        assert_eq!(
            writes,
            vec![
                SdoWrite {
                    subdevice_address: 0x1003,
                    index: 0x8010,
                    subindex: 0x01,
                    value: SdoValue::U16(1800)
                },
                SdoWrite {
                    subdevice_address: 0x1003,
                    index: 0x8010,
                    subindex: 0x02,
                    value: SdoValue::U16(900)
                },
            ]
        );
    }

    /// `TEST_0869` — an empty `startup_sdos` list produces zero startup SDO
    /// writes (`REQ_0853`).
    #[test]
    fn empty_startup_sdos_produce_no_writes() {
        let map = SubDeviceMap::new(0x1003, &[], &[], 3);
        assert!(startup_sdo_writes(&map).is_empty());
    }

    #[test]
    fn startup_writes_carry_wide_and_signed_values() {
        static STARTUP: &[StartupSdo] = &[
            StartupSdo {
                index: 0x8011,
                subindex: 0x01,
                value: SdoValue::U32(100_000),
            },
            StartupSdo {
                index: 0x8011,
                subindex: 0x02,
                value: SdoValue::I32(-5),
            },
            StartupSdo {
                index: 0x8011,
                subindex: 0x03,
                value: SdoValue::I16(-1),
            },
            StartupSdo {
                index: 0x8011,
                subindex: 0x04,
                value: SdoValue::I8(-128),
            },
        ];
        let map = SubDeviceMap::new(0x1003, &[], &[], 3).with_startup_sdos(STARTUP);
        let writes = startup_sdo_writes(&map);
        assert_eq!(writes.len(), 4);
        assert_eq!(writes[0].value, SdoValue::U32(100_000));
        assert_eq!(writes[1].value, SdoValue::I32(-5));
        assert_eq!(writes[2].value, SdoValue::I16(-1));
        assert_eq!(writes[3].value, SdoValue::I8(-128));
    }
}
