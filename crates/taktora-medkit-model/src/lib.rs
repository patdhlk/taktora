//! Wire DTOs for the SOVD-aligned `taktora-medkit` runtime-diagnostics surface.
//!
//! These types are the wire contract in code: the SOVD entity tree
//! (Area / Component / Function / App) and its collection envelope, the
//! DTC / fault model with its SOVD/UDS-style status sub-object and freeze-frame
//! environment data, the fault-event stream payload, and the generic error
//! envelope — satisfying [`REQ_0911`], [`REQ_0914`], and [`REQ_0915`].
//!
//! This is a clean-room model of the diagnostic *contract* behind
//! `selfpatch/ros2_medkit`; it carries **zero** taktora dependencies so the
//! diagnostics core stays extractable ([`REQ_0916`], `ADR_0111`).
//!
//! The serialized shape is pinned **byte-for-byte** against the captured
//! `ros2_medkit` corpus under `contract/golden/*.json` by the snapshot tests in
//! this crate ([`TEST_0905`]): each golden fixture is deserialized into the
//! model type and re-serialized, and the result must equal the fixture
//! key-for-key and value-for-value. Drop-in client compatibility is therefore a
//! failing test, not a field report.
//!
//! ## Mixed casing is the contract, not a bug
//!
//! The corpus deliberately mixes `snake_case` and `camelCase`, and this model
//! mirrors it via `#[serde(rename = "...")]` rather than reshaping the ergonomic
//! Rust field names:
//!
//! - Envelope / fault bodies are `snake_case` (`fault_code`, `occurrence_count`,
//!   `reporting_sources`, `severity_label`).
//! - The DTC status sub-object is camelCase (`aggregatedStatus`, `testFailed`,
//!   `confirmedDTC`, `pendingDTC`) with **string** `"1"` / `"0"` values,
//!   mirroring the SOVD/UDS status byte.
//! - Collections carry `items` plus an `x-medkit` extension object (and, for
//!   relationship sub-resources, HATEOAS `_links`).
//!
//! [`REQ_0911`]: https://taktora.dev/requirements/medkit/index.html
//! [`REQ_0914`]: https://taktora.dev/requirements/medkit/index.html
//! [`REQ_0915`]: https://taktora.dev/requirements/medkit/index.html
//! [`REQ_0916`]: https://taktora.dev/requirements/medkit/index.html
//! [`TEST_0905`]: https://taktora.dev/verification/medkit/index.html

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Kind of SOVD entity in the diagnostic tree (`REQ_0914`).
///
/// Serializes to the contract's lowercase `type` discriminator (`"app"`,
/// `"component"`, `"function"`, `"area"`).
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

/// Fault severity, ordered least-to-most severe so a worst-wins rollup is a
/// simple `max` (`REQ_0912`).
///
/// This is an ergonomic domain helper for the health rollup; the wire fault
/// shapes carry the raw numeric `severity` plus a human `severity_label` string
/// directly (see [`FaultSummary`] and [`FaultItem`]). The corpus attests the
/// numeric values `1` (`WARN`) and `2` (`ERROR`); the full mapping below follows
/// the SOVD/UDS severity ordering.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    /// Informational; not a fault. Wire value `0`.
    Info,
    /// Warning-level fault. Wire value `1`.
    Warn,
    /// Error-level fault. Wire value `2`.
    Error,
    /// Critical fault. Wire value `3`.
    Critical,
}

impl Severity {
    /// The numeric wire value the contract uses for this severity.
    #[must_use]
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Warn => 1,
            Self::Error => 2,
            Self::Critical => 3,
        }
    }

    /// Interpret a numeric wire `severity` value, if it is a known level.
    #[must_use]
    pub const fn from_wire_value(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Info),
            1 => Some(Self::Warn),
            2 => Some(Self::Error),
            3 => Some(Self::Critical),
            _ => None,
        }
    }
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

/// The `ros2` sub-object inside an entity's `x-medkit` extension: the backing
/// ROS 2 node FQN.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Ros2Ref {
    /// The ROS 2 node fully-qualified name (e.g. `/ros2_medkit_gateway`).
    pub node: String,
}

/// The `x-medkit` extension carried on an entity summary item.
///
/// All fields are optional because the gateway decorates entities differently by
/// context: top-level `/apps` items carry `component_id`, relationship
/// sub-resource items omit it, and discovered `/components` / `/functions` items
/// carry only `source`.
#[derive(Clone, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct EntityMeta {
    /// Owning component id, when the entity is an app surfaced under a component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    /// Whether the backing node is currently online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_online: Option<bool>,
    /// Backing ROS 2 node reference, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ros2: Option<Ros2Ref>,
    /// How the entity was discovered (e.g. `heuristic`, `runtime`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A node in the SOVD entity tree as served in a collection (`REQ_0914`).
///
/// This is the summary shape the gateway emits inside collection `items`: a
/// stable `id`, a human-readable `name`, the entity `type`, a self `href`, and an
/// optional `x-medkit` decoration. The verbose single-entity detail view (the
/// capability catalogue with flattened relationship links) is a separate
/// server-rendered shape and is not modelled here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Self link for the entity (e.g. `/api/v1/apps/ros2_medkit_gateway`).
    pub href: String,
    /// Stable identifier, unique within the tree.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The kind of entity, serialized as the contract's `type` discriminator.
    #[serde(rename = "type")]
    pub kind: EntityKind,
    /// Parent entity id, for the internal worst-wins health rollup (`REQ_0912`).
    ///
    /// **Not part of the wire corpus**: every served entity expresses hierarchy
    /// through relationship sub-resources / `_links`, never an inline parent
    /// field, so this is absent (`None`) for anything deserialized from the
    /// contract and is skipped on serialization. The gateway populates it only
    /// to drive the rollup over the provider seam.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Optional human-readable description (e.g. host OS for a component).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The `x-medkit` extension object, when the gateway decorates the entity.
    #[serde(rename = "x-medkit", skip_serializing_if = "Option::is_none")]
    pub x_medkit: Option<EntityMeta>,
}

/// The `x-medkit` extension carried on a collection envelope: the total item
/// count.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollectionMeta {
    /// Total number of items in the collection (before any paging).
    pub total_count: usize,
}

/// A SOVD collection envelope: an `items` array plus the `x-medkit` extension
/// (`total_count`) and, for relationship sub-resources, HATEOAS `_links`.
///
/// Top-level entity lists (`/apps`, `/components`, `/functions`, `/areas`) carry
/// `items` + `x-medkit.total_count` with no `_links`; relationship sub-resources
/// (e.g. `…/hosts`, `…/is-located-on`) add a `_links` map. Fault lists use a
/// *different* envelope ([`FaultList`]) whose extension keys diverge, so they are
/// deliberately not modelled as a `Collection`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Collection<T> {
    /// The collected items.
    pub items: Vec<T>,
    /// HATEOAS link map (`self`, plus the parent relationship link), when present.
    #[serde(rename = "_links", skip_serializing_if = "Option::is_none")]
    pub links: Option<BTreeMap<String, String>>,
    /// The `x-medkit` collection extension.
    #[serde(rename = "x-medkit")]
    pub x_medkit: CollectionMeta,
}

impl<T> Collection<T> {
    /// Wrap `items` in a top-level collection envelope (no `_links`), setting
    /// `total_count` to its length.
    #[must_use]
    pub fn new(items: Vec<T>) -> Self {
        let total_count = items.len();
        Self {
            items,
            links: None,
            x_medkit: CollectionMeta { total_count },
        }
    }
}

impl<T> FromIterator<T> for Collection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

/// A fault as served in a fault *list* item (`REQ_0915`).
///
/// `snake_case` throughout: `fault_code`, `occurrence_count`, `reporting_sources`,
/// `severity_label`. The numeric `severity` pairs with the human `severity_label`
/// string; the raw lifecycle `status` (e.g. `CONFIRMED`, `PREFAILED`) is a plain
/// string here (the structured DTC status sub-object appears only in the fault
/// detail, see [`FaultItem`]).
// `f64` occurrence timestamps preclude an `Eq` derive.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaultSummary {
    /// Human-readable fault description.
    pub description: String,
    /// Global fault code identifier (e.g. `BRAKE_PRESSURE_LOW`).
    pub fault_code: String,
    /// First occurrence, epoch seconds (fractional).
    pub first_occurred: f64,
    /// Last occurrence, epoch seconds (fractional).
    pub last_occurred: f64,
    /// Aggregated occurrences across all reporting sources.
    pub occurrence_count: u32,
    /// Identifiers of every source reporting this fault (ROS node FQNs).
    pub reporting_sources: Vec<String>,
    /// Numeric severity (see [`Severity`] for the level mapping).
    pub severity: u8,
    /// Human-readable severity label (e.g. `ERROR`, `WARN`).
    pub severity_label: String,
    /// Raw fault lifecycle status (e.g. `CONFIRMED`, `PREFAILED`).
    pub status: String,
}

/// The `x-medkit` extension carried on a fault-list envelope.
///
/// Only `count` is always present. The default `/faults` and filtered lists add
/// `muted_count` / `cluster_count`; app-scoped lists add `entity_id` /
/// `source_id`; component-scoped (aggregated) lists carry the aggregation
/// metadata (`aggregated`, `aggregation_level`, `aggregation_sources`,
/// `app_count`, `entity_id`) instead.
#[derive(Clone, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct FaultListMeta {
    /// Number of faults in this list.
    pub count: u32,
    /// Number of muted faults (default / app-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted_count: Option<u32>,
    /// Number of fault clusters (default / app-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_count: Option<u32>,
    /// Scoping entity id (app- and component-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// Scoping source id / node FQN (app-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Whether this list aggregates across owned apps (component-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregated: Option<bool>,
    /// Aggregation level (e.g. `component`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregation_level: Option<String>,
    /// The app FQNs aggregated over (component-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregation_sources: Option<Vec<String>>,
    /// Number of apps aggregated (component-scoped lists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_count: Option<u32>,
}

/// A fault-list envelope: `items` of [`FaultSummary`] plus the fault-list
/// `x-medkit` extension ([`FaultListMeta`]).
///
/// This is a distinct envelope from [`Collection`]: fault lists key their count
/// under `x-medkit.count` (not `total_count`) and carry muted/cluster/aggregation
/// metadata.
// Holds `FaultSummary`, whose `f64` timestamps preclude an `Eq` derive.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaultList {
    /// The faults in this list.
    pub items: Vec<FaultSummary>,
    /// The `x-medkit` fault-list extension.
    #[serde(rename = "x-medkit")]
    pub x_medkit: FaultListMeta,
}

/// SOVD/UDS-style DTC status sub-object, nested under a fault detail's `item`.
///
/// Field names are camelCase per the contract and **values are strings**:
/// `aggregatedStatus` is a label (e.g. `active`), while `testFailed`,
/// `confirmedDTC`, and `pendingDTC` are `"1"` / `"0"` mirroring the UDS status
/// byte (`REQ_0911`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DtcStatus {
    /// Aggregated status label (e.g. `active`).
    #[serde(rename = "aggregatedStatus")]
    pub aggregated_status: String,
    /// Whether the most recent test failed (`"1"` / `"0"`).
    #[serde(rename = "testFailed")]
    pub test_failed: String,
    /// Whether the DTC is confirmed (`"1"` / `"0"`).
    #[serde(rename = "confirmedDTC")]
    pub confirmed_dtc: String,
    /// Whether the DTC is pending (`"1"` / `"0"`).
    #[serde(rename = "pendingDTC")]
    pub pending_dtc: String,
}

/// The `item` sub-object of a fault detail: the DTC identity, severity, and
/// status sub-object (`REQ_0915`).
///
/// Note the detail view renames the fault code to `code` and the description to
/// `fault_name` (versus the `snake_case` [`FaultSummary`] used in lists).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FaultItem {
    /// Global fault code identifier (e.g. `BRAKE_PRESSURE_LOW`).
    pub code: String,
    /// Human-readable fault name / description.
    pub fault_name: String,
    /// Numeric severity (see [`Severity`] for the level mapping).
    pub severity: u8,
    /// SOVD/UDS status sub-object.
    pub status: DtcStatus,
}

/// First / last occurrence bookkeeping for a fault, as ISO-8601 timestamps
/// (`REQ_0915`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExtendedDataRecords {
    /// First occurrence, ISO-8601 (e.g. `2026-06-27T22:40:00.250Z`).
    pub first_occurrence: String,
    /// Last occurrence, ISO-8601.
    pub last_occurrence: String,
}

/// The `x-medkit` extension carried on a freeze-frame snapshot.
///
/// `D` is the freeze-frame payload type (the captured system state); the corpus
/// carries an arbitrary JSON object, so callers pick the representation
/// (e.g. `serde_json::Value`). Keeping the payload generic preserves the crate's
/// serde-only, zero-dependency invariant (`REQ_0916`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FreezeFrameMeta<D> {
    /// Capture time, ISO-8601.
    pub captured_at: String,
    /// The full captured payload.
    pub full_data: D,
    /// ROS 2 message type of the captured topic.
    pub message_type: String,
    /// ROS 2 topic the snapshot was captured from.
    pub topic: String,
}

/// A freeze-frame / snapshot of system state captured at fault time
/// (`REQ_0915`).
///
/// Lives in [`EnvironmentData::snapshots`]; the `type` discriminator is
/// `freeze_frame`. `D` is the captured-payload type (see [`FreezeFrameMeta`]).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FreezeFrame<D> {
    /// The captured payload.
    pub data: D,
    /// Human-readable label for the snapshot.
    pub name: String,
    /// Snapshot discriminator (e.g. `freeze_frame`).
    #[serde(rename = "type")]
    pub kind: String,
    /// The `x-medkit` snapshot extension (capture metadata + full payload).
    #[serde(rename = "x-medkit")]
    pub x_medkit: FreezeFrameMeta<D>,
}

/// Environment data attached to a fault detail: occurrence records plus
/// freeze-frame snapshots (`REQ_0915`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentData<D> {
    /// First / last occurrence records.
    pub extended_data_records: ExtendedDataRecords,
    /// Zero or more freeze-frame captures.
    pub snapshots: Vec<FreezeFrame<D>>,
}

/// The `x-medkit` extension carried on a fault detail.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FaultDetailMeta {
    /// Aggregated occurrences across all reporting sources.
    pub occurrence_count: u32,
    /// Identifiers of every source reporting this fault.
    pub reporting_sources: Vec<String>,
    /// Human-readable severity label (e.g. `ERROR`).
    pub severity_label: String,
    /// Raw lifecycle status (e.g. `CONFIRMED`).
    pub status_raw: String,
}

/// A single fault / DTC detail: environment data (occurrence records +
/// freeze-frames), the DTC `item` with its status sub-object, and the `x-medkit`
/// extension (`REQ_0915`).
///
/// `D` is the freeze-frame payload type (see [`FreezeFrame`]).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FaultDetail<D> {
    /// Occurrence records and freeze-frame snapshots.
    pub environment_data: EnvironmentData<D>,
    /// The DTC identity, severity, and status sub-object.
    pub item: FaultItem,
    /// The `x-medkit` fault-detail extension.
    #[serde(rename = "x-medkit")]
    pub x_medkit: FaultDetailMeta,
}

/// The `x-medkit` extension carried on a fault-stream event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FaultEventMeta {
    /// The entity the fault is scoped to.
    pub entity_id: String,
    /// The entity-collection type (e.g. `apps`).
    pub entity_type: String,
}

/// A fault-stream event: the `data:` payload of a `/faults/stream` SSE frame.
///
/// Carries the event kind (e.g. `fault_confirmed`), the [`FaultSummary`], an
/// epoch-seconds `timestamp`, and the `x-medkit` scoping extension (`REQ_0911`).
// `f64` timestamp and `FaultSummary` payload preclude an `Eq` derive.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaultEvent {
    /// The event kind (e.g. `fault_confirmed`).
    pub event_type: String,
    /// The fault payload.
    pub fault: FaultSummary,
    /// Event time, epoch seconds (fractional).
    pub timestamp: f64,
    /// The `x-medkit` event-scoping extension.
    #[serde(rename = "x-medkit")]
    pub x_medkit: FaultEventMeta,
}

/// The gateway's generic error envelope (`REQ_0911`).
///
/// Emitted for error responses (e.g. `entity-not-found`): a machine-readable
/// `error_code`, a human `message`, and a string-keyed `parameters` map of
/// context (e.g. the offending `entity_id`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GenericError {
    /// Machine-readable error code (e.g. `entity-not-found`).
    pub error_code: String,
    /// Human-readable error message.
    pub message: String,
    /// Contextual parameters (e.g. the offending `entity_id`).
    pub parameters: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use serde_json::Value;

    use super::*;

    /// Deserialize a `contract/golden` fixture into model type `T`, re-serialize
    /// it, and assert the round-tripped JSON equals the fixture key-for-key and
    /// value-for-value. Comparison is over parsed `serde_json::Value`, so object
    /// key *order* is irrelevant but every key, casing, and value must match —
    /// proving the model represents the contract losslessly (`TEST_0905`).
    fn assert_golden_snapshot<T>(fixture: &str)
    where
        T: Serialize + DeserializeOwned,
    {
        let path = format!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/golden/{}"),
            fixture
        );
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read golden fixture {path}: {e}"));
        let golden: Value = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("parse golden fixture {fixture}: {e}"));
        let model: T = serde_json::from_value(golden.clone())
            .unwrap_or_else(|e| panic!("deserialize {fixture} into model: {e}"));
        let round_tripped =
            serde_json::to_value(&model).unwrap_or_else(|e| panic!("re-serialize {fixture}: {e}"));
        assert_eq!(
            round_tripped, golden,
            "model serialization diverged from golden fixture {fixture}"
        );
    }

    /// `TEST_0905` — entity collection envelopes (top-level lists + relationship
    /// sub-resources) round-trip the corpus exactly: `items`, optional `_links`,
    /// and `x-medkit.total_count` (`REQ_0911`, `REQ_0914`).
    #[test]
    fn entity_collections_match_golden() {
        for fixture in [
            "apps_list.json",
            "components_list.json",
            "functions_list.json",
            "areas_list.json",
            "component_hosts.json",
            "function_hosts.json",
            "collection_envelope_example.json",
            "app_is-located-on.json",
            "app_belongs-to.json",
            "app_depends-on.json",
            "component_depends-on.json",
            "component_subcomponents.json",
        ] {
            assert_golden_snapshot::<Collection<Entity>>(fixture);
        }
    }

    /// `TEST_0905` — fault-list envelopes round-trip the corpus exactly across
    /// the default, filtered, app-scoped, and component-aggregated shapes:
    /// `snake_case` `fault_code` items plus the `x-medkit` count/aggregation
    /// extension (`REQ_0911`, `REQ_0915`).
    #[test]
    fn fault_lists_match_golden() {
        for fixture in [
            "faults_list.json",
            "faults_filtered_pending.json",
            "app_faults_list.json",
            "component_faults_list.json",
        ] {
            assert_golden_snapshot::<FaultList>(fixture);
        }
    }

    /// `TEST_0905` — the single-fault detail round-trips exactly: the camelCase
    /// DTC status sub-object, `extended_data_records`, and the `snapshots`
    /// freeze-frame with its `x-medkit` capture metadata (`REQ_0911`,
    /// `REQ_0915`). The freeze-frame payload is modelled as `serde_json::Value`.
    #[test]
    fn fault_detail_matches_golden() {
        assert_golden_snapshot::<FaultDetail<Value>>("fault_get_with_freezeframe.json");
    }

    /// `TEST_0905` — the fault-stream event payload round-trips exactly
    /// (`REQ_0911`).
    #[test]
    fn fault_event_matches_golden() {
        assert_golden_snapshot::<FaultEvent>("faults_stream_event.json");
    }

    /// `TEST_0905` — the generic error envelope round-trips exactly
    /// (`REQ_0911`).
    #[test]
    fn error_envelope_matches_golden() {
        assert_golden_snapshot::<GenericError>("error_not_found.json");
    }

    /// `TEST_0905` — the served fault shapes carry the contract's mandatory keys
    /// and casing, and none of the scaffold's legacy names survive serialization
    /// (`REQ_0911`).
    #[test]
    fn fault_shapes_use_contract_keys() {
        let detail_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contract/golden/fault_get_with_freezeframe.json"
        );
        let detail: FaultDetail<Value> =
            serde_json::from_str(&fs::read_to_string(detail_path).unwrap()).unwrap();
        let detail_json = serde_json::to_string(&detail).unwrap();
        for present in [
            "aggregatedStatus",
            "testFailed",
            "confirmedDTC",
            "pendingDTC",
            "snapshots",
            "extended_data_records",
        ] {
            assert!(
                detail_json.contains(present),
                "fault detail missing {present}"
            );
        }

        let list_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contract/golden/faults_list.json"
        );
        let list: FaultList =
            serde_json::from_str(&fs::read_to_string(list_path).unwrap()).unwrap();
        let list_json = serde_json::to_string(&list).unwrap();
        assert!(
            list_json.contains("fault_code"),
            "fault list missing fault_code"
        );

        // No scaffold-era legacy names survive in either serialized shape.
        for legacy in ["freeze_frames", "\"code\"", "confirmed\"", "pending\""] {
            assert!(
                !list_json.contains(legacy),
                "legacy name {legacy} leaked into fault list output"
            );
        }
        assert!(
            !detail_json.contains("freeze_frames"),
            "legacy name freeze_frames leaked into fault detail output"
        );
    }

    /// `TEST_0900` — the collection envelope helper round-trips and counts items.
    #[test]
    fn collection_helper_round_trips() {
        let collection: Collection<Entity> = Collection::new(vec![Entity {
            href: "/api/v1/apps/bt_navigator".to_owned(),
            id: "bt_navigator".to_owned(),
            name: "bt_navigator".to_owned(),
            kind: EntityKind::App,
            parent_id: None,
            description: None,
            x_medkit: None,
        }]);
        assert_eq!(collection.x_medkit.total_count, 1);
        let json = serde_json::to_string(&collection).unwrap();
        let back: Collection<Entity> = serde_json::from_str(&json).unwrap();
        assert_eq!(collection, back);
    }

    /// `REQ_0912` — worst-wins ordering holds on `Health` so the gateway rollup
    /// is a `max`.
    #[test]
    fn health_orders_worst_wins() {
        assert!(Health::Critical > Health::Error);
        assert!(Health::Error > Health::Warning);
        assert!(Health::Warning > Health::Ok);
    }

    /// `REQ_0912` — `Severity` maps to and from its numeric wire value.
    #[test]
    fn severity_wire_value_round_trips() {
        for severity in [
            Severity::Info,
            Severity::Warn,
            Severity::Error,
            Severity::Critical,
        ] {
            assert_eq!(
                Severity::from_wire_value(severity.wire_value()),
                Some(severity)
            );
        }
        assert_eq!(Severity::from_wire_value(9), None);
    }
}
