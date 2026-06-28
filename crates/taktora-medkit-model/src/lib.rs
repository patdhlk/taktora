//! Wire DTOs for the SOVD-aligned `taktora-medkit` runtime-diagnostics surface.
//!
//! These types are the wire contract in code: the entity tree
//! (Area / Component / Function / App), the DTC / fault model with its
//! SOVD/UDS-style status sub-object and freeze-frame environment data, and the
//! reusable collection envelope — satisfying [`REQ_0914`] and [`REQ_0915`].
//!
//! This is a clean-room model of the diagnostic *contract* behind
//! `selfpatch/ros2_medkit`; it carries **zero** taktora dependencies so the
//! diagnostics core stays extractable ([`REQ_0916`], `ADR_0111`).
//!
//! Byte-for-byte alignment with the captured `ros2_medkit` corpus (exact field
//! casing, HATEOAS `_links`, the `x-medkit` envelope) is owned by a downstream
//! slice; this grounding scaffold fixes the shape and the requirement IDs.
//!
//! [`REQ_0914`]: https://taktora.dev/requirements/medkit/index.html
//! [`REQ_0915`]: https://taktora.dev/requirements/medkit/index.html
//! [`REQ_0916`]: https://taktora.dev/requirements/medkit/index.html

use serde::{Deserialize, Serialize};

/// Kind of SOVD entity in the diagnostic tree (`REQ_0914`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityKind {
    /// Physical or logical domain grouping.
    Area,
    /// Logical grouping of apps; the unit health rolls up over.
    Component,
    /// A logical capability spanning apps.
    Function,
    /// A single running process / node.
    App,
}

/// A node in the SOVD entity tree (`REQ_0914`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Stable identifier, unique within the tree.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The kind of entity.
    pub kind: EntityKind,
    /// Identifier of the parent entity, if any (`None` for roots).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

/// Fault severity, ordered least-to-most severe so a worst-wins rollup is a
/// simple `max` (`REQ_0912`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    /// Informational; not a fault.
    Info,
    /// Warning-level fault.
    Warn,
    /// Error-level fault.
    Error,
    /// Critical fault.
    Critical,
}

/// Aggregated health of an entity, ordered least-to-most severe so the
/// worst-wins rollup is a `max` over an entity and its descendants (`REQ_0912`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Health {
    /// No active fault.
    Ok,
    /// At least one warning, no error or worse.
    Warning,
    /// At least one error, no critical.
    Error,
    /// At least one critical fault.
    Critical,
}

/// SOVD/UDS-style DTC status sub-object. Field names mirror the `ros2_medkit`
/// contract's camelCase status bits; exact corpus alignment is a downstream
/// slice (`REQ_0911`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DtcStatus {
    /// Aggregated status byte, as a human-readable label.
    #[serde(rename = "aggregatedStatus")]
    pub aggregated_status: String,
    /// Whether the most recent test failed.
    #[serde(rename = "testFailed")]
    pub test_failed: bool,
    /// Whether the DTC is confirmed.
    #[serde(rename = "confirmedDTC")]
    pub confirmed_dtc: bool,
    /// Whether the DTC is pending.
    #[serde(rename = "pendingDTC")]
    pub pending_dtc: bool,
}

/// A freeze-frame / snapshot of system state captured at fault time
/// (`REQ_0915`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FreezeFrame {
    /// Human-readable label for the snapshot.
    pub name: String,
    /// Capture time, nanoseconds since the Unix epoch.
    pub captured_at_ns: i64,
    /// Captured payload, as a serialized JSON string.
    pub data: String,
}

/// First/last occurrence bookkeeping for a fault (`REQ_0915`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExtendedDataRecords {
    /// First occurrence, nanoseconds since the Unix epoch.
    pub first_occurrence_ns: i64,
    /// Last occurrence, nanoseconds since the Unix epoch.
    pub last_occurrence_ns: i64,
}

/// Environment data attached to a fault: occurrence records plus freeze-frames
/// (`REQ_0915`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentData {
    /// First/last occurrence records.
    pub extended_data_records: ExtendedDataRecords,
    /// Zero or more freeze-frame captures.
    pub snapshots: Vec<FreezeFrame>,
}

/// A diagnostic trouble code: the medkit fault model (`REQ_0915`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Dtc {
    /// Global fault code identifier (e.g. `MOTOR_OVERHEAT`).
    pub fault_code: String,
    /// SOVD status sub-object.
    pub status: DtcStatus,
    /// Severity label.
    pub severity: Severity,
    /// Aggregated occurrences across all reporting sources.
    pub occurrence_count: u32,
    /// Identifiers of every source reporting this fault.
    pub reporting_sources: Vec<String>,
    /// Occurrence records and freeze-frames.
    pub environment_data: EnvironmentData,
}

/// A collection envelope wrapping a list of items.
///
/// The `ros2_medkit` contract additionally carries HATEOAS `_links` and an
/// `x-medkit` object; modelling those exactly is a downstream slice
/// (`REQ_0911`). This grounding shape carries `items` and `total_count`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Collection<T> {
    /// The collected items.
    pub items: Vec<T>,
    /// Total number of items (before any paging).
    pub total_count: usize,
}

impl<T> Collection<T> {
    /// Wrap `items` in a collection envelope, setting `total_count` to its length.
    #[must_use]
    pub fn new(items: Vec<T>) -> Self {
        let total_count = items.len();
        Self { items, total_count }
    }
}

impl<T> FromIterator<T> for Collection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dtc() -> Dtc {
        Dtc {
            fault_code: "MOTOR_OVERHEAT".to_owned(),
            status: DtcStatus {
                aggregated_status: "CONFIRMED".to_owned(),
                test_failed: true,
                confirmed_dtc: true,
                pending_dtc: false,
            },
            severity: Severity::Error,
            occurrence_count: 3,
            reporting_sources: vec!["/motor".to_owned()],
            environment_data: EnvironmentData {
                extended_data_records: ExtendedDataRecords {
                    first_occurrence_ns: 1,
                    last_occurrence_ns: 9,
                },
                snapshots: vec![FreezeFrame {
                    name: "at-abort".to_owned(),
                    captured_at_ns: 9,
                    data: "{\"temp\":91}".to_owned(),
                }],
            },
        }
    }

    /// `TEST_0900` — model wire shapes survive a serialize -> deserialize round-trip.
    #[test]
    fn entity_round_trips() {
        let entity = Entity {
            id: "app:bt_navigator".to_owned(),
            name: "bt_navigator".to_owned(),
            kind: EntityKind::App,
            parent_id: Some("component:nav".to_owned()),
        };
        let json = serde_json::to_string(&entity).unwrap();
        let back: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(entity, back);
    }

    /// `TEST_0900` — a DTC with its status sub-object and a freeze-frame round-trips.
    #[test]
    fn dtc_round_trips() {
        let dtc = sample_dtc();
        let json = serde_json::to_string(&dtc).unwrap();
        let back: Dtc = serde_json::from_str(&json).unwrap();
        assert_eq!(dtc, back);
    }

    /// `TEST_0900` — the DTC status sub-object serializes to the contract's
    /// camelCase keys (the grounding subset of `REQ_0911`).
    #[test]
    fn dtc_status_uses_contract_casing() {
        let json = serde_json::to_string(&sample_dtc()).unwrap();
        assert!(json.contains("\"fault_code\""));
        assert!(json.contains("\"aggregatedStatus\""));
        assert!(json.contains("\"confirmedDTC\""));
        assert!(json.contains("\"pendingDTC\""));
    }

    /// `TEST_0900` — the collection envelope round-trips and counts its items.
    #[test]
    fn collection_round_trips() {
        let collection: Collection<Entity> = Collection::new(vec![Entity {
            id: "area:root".to_owned(),
            name: "root".to_owned(),
            kind: EntityKind::Area,
            parent_id: None,
        }]);
        assert_eq!(collection.total_count, 1);
        let json = serde_json::to_string(&collection).unwrap();
        let back: Collection<Entity> = serde_json::from_str(&json).unwrap();
        assert_eq!(collection, back);
    }

    /// Worst-wins ordering holds on `Health` so the gateway rollup is a `max`.
    #[test]
    fn health_orders_worst_wins() {
        assert!(Health::Critical > Health::Error);
        assert!(Health::Error > Health::Warning);
        assert!(Health::Warning > Health::Ok);
    }
}
