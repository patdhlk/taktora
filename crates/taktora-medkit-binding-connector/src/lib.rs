//! Connector-framework binding for `taktora-medkit`.
//!
//! This crate is one of the two seams where taktora coupling is quarantined
//! (`ADR_0111`, `ADR_0115`). It ingests a connector's [`ConnectorHealth`]
//! transition stream and maps it into the medkit [`Provider`] seam, producing a
//! [`ProviderSnapshot`] with a SOVD Component (the bridge / `SubDevice` standing
//! in for the connector) and the DTCs its health implies — off the control path
//! (`REQ_0910`), satisfying `REQ_0926`, `REQ_0927`, and `REQ_0928`.
//!
//! # Health → DTC mapping (`REQ_0926`)
//!
//! | Connector health    | Component health | DTC raised                                |
//! |---------------------|------------------|-------------------------------------------|
//! | `Up`                | `Ok`             | none (active DTCs healed)                 |
//! | `Connecting`        | `Warning`        | none (fault condition persists)           |
//! | `Degraded{reason}`  | `Warning`        | `FIELDBUS_DEGRADED` (Warn), reason carried |
//! | `Down{reason}`      | `Critical`       | `FIELDBUS_NOT_OPERATIONAL` (Critical)     |
//!
//! Reasons are read as **strings** off the health variant — the connector
//! surface does not expose a typed fault enum.
//!
//! # Ingestion model (`REQ_0910`, `REQ_0913`)
//!
//! `taktora-connector-core` exposes health per-connector and has no
//! `subscribe_health()`; the binding therefore models its input as a health
//! **event stream** it ingests through [`MedkitProvider::on_health_event`]. A
//! real per-connector health surface drives that method from its off-path drain;
//! tests drive it with a simulated transition sequence. Because the
//! [`Provider`] is read with `&self` from the gateway's request path, the store
//! lives behind interior mutability, so a callback can write while the gateway
//! reads.
//!
//! # Freeze-frame (`REQ_0928`)
//!
//! v1 is callback-hooks-only (no iceoryx2 PDI slice — `REQ_0913`): the
//! freeze-frame captured at DTC confirmation is the **last hook sample**
//! observed via [`MedkitProvider::observe_sample`], or, absent any sample, a
//! synthesized snapshot of the health transition (state + reason). It is
//! rendered under the contract's `snapshots` / `extended_data_records` shape.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use taktora_connector_core::health::{ConnectorHealth, ConnectorHealthKind, HealthEvent};
use taktora_medkit_model::{
    DtcStatus, Entity, EntityKind, EntityMeta, EnvironmentData, ExtendedDataRecords, FaultDetail,
    FaultDetailMeta, FaultItem, FaultSummary, FreezeFrame, FreezeFrameMeta, Health, Severity,
};
use taktora_medkit_provider::{Provider, ProviderSnapshot};

/// DTC code raised while the connector is `Down` (`REQ_0926`).
pub const DTC_NOT_OPERATIONAL: &str = "FIELDBUS_NOT_OPERATIONAL";
/// DTC code raised while the connector is `Degraded` (`REQ_0926`).
pub const DTC_DEGRADED: &str = "FIELDBUS_DEGRADED";

/// The ROS-2-message-type and topic labels stamped onto a freeze-frame capture,
/// naming the connector health surface the sample came from (`REQ_0928`).
const FREEZE_FRAME_MESSAGE_TYPE: &str = "taktora_connector_core/ConnectorHealth";
const FREEZE_FRAME_TOPIC: &str = "connector/health";

/// Map a connector health discriminator to the medkit [`Health`] it implies for
/// the Component standing in for that connector (`REQ_0912`, `REQ_0926`).
///
/// This is the *state-only* projection; the Component's reported health is the
/// worst of this and any active DTC (a `Down` DTC is Critical, one step above
/// the `Error` the bare `Down` state implies).
#[must_use]
pub const fn map_health(kind: ConnectorHealthKind) -> Health {
    match kind {
        ConnectorHealthKind::Up => Health::Ok,
        // A connector that is reconnecting or degraded is a warning, not yet a
        // hard fault: the bus may recover without intervention.
        ConnectorHealthKind::Connecting | ConnectorHealthKind::Degraded => Health::Warning,
        ConnectorHealthKind::Down => Health::Error,
    }
}

/// Map a fault [`Severity`] to the [`Health`] it implies.
const fn severity_to_health(severity: Severity) -> Health {
    match severity {
        Severity::Info => Health::Ok,
        Severity::Warn => Health::Warning,
        Severity::Error => Health::Error,
        Severity::Critical => Health::Critical,
    }
}

/// In-memory DTC record: identity, severity, status bits, occurrence
/// bookkeeping, and the freeze-frame captured at confirmation (`REQ_0927`,
/// `REQ_0928`).
#[derive(Clone, Debug)]
struct DtcRecord {
    code: &'static str,
    severity: Severity,
    description: String,
    reason: String,
    /// `testFailed` — the fault condition is currently present.
    active: bool,
    /// `confirmedDTC` — the fault has been confirmed at least once. UDS DTC
    /// memory keeps this set after the condition clears (healed, not erased).
    confirmed: bool,
    occurrence_count: u32,
    first_occurred: f64,
    last_occurred: f64,
    /// Last hook sample captured at the most recent confirmation.
    freeze_frame: Value,
    captured_at: f64,
}

impl DtcRecord {
    const fn new(code: &'static str, severity: Severity) -> Self {
        Self {
            code,
            severity,
            description: String::new(),
            reason: String::new(),
            active: false,
            confirmed: false,
            occurrence_count: 0,
            first_occurred: 0.0,
            last_occurred: 0.0,
            freeze_frame: Value::Null,
            captured_at: 0.0,
        }
    }

    /// Raw lifecycle status string (the `snake_case` fault-list `status`).
    const fn status_raw(&self) -> &'static str {
        if self.active {
            "CONFIRMED"
        } else if self.confirmed {
            "HEALED"
        } else {
            "PENDING"
        }
    }

    fn status_bits(&self) -> DtcStatus {
        DtcStatus {
            aggregated_status: if self.active { "active" } else { "healed" }.to_owned(),
            test_failed: bool01(self.active),
            confirmed_dtc: bool01(self.confirmed),
            // v1 confirms on the first callback sample — there is no multi-cycle
            // pending window, so `pendingDTC` is never latched.
            pending_dtc: bool01(false),
        }
    }

    fn summary(&self) -> FaultSummary {
        FaultSummary {
            description: self.description.clone(),
            fault_code: self.code.to_owned(),
            first_occurred: self.first_occurred,
            last_occurred: self.last_occurred,
            occurrence_count: self.occurrence_count,
            reporting_sources: Vec::new(),
            severity: self.severity.wire_value(),
            severity_label: format!("{:?}", self.severity).to_uppercase(),
            status: self.status_raw().to_owned(),
        }
    }

    fn freeze_frames(&self) -> Vec<FreezeFrame<Value>> {
        if !self.confirmed {
            return Vec::new();
        }
        vec![FreezeFrame {
            data: self.freeze_frame.clone(),
            name: "Connector health at confirmation".to_owned(),
            kind: "freeze_frame".to_owned(),
            x_medkit: FreezeFrameMeta {
                captured_at: iso8601(self.captured_at),
                full_data: self.freeze_frame.clone(),
                message_type: FREEZE_FRAME_MESSAGE_TYPE.to_owned(),
                topic: FREEZE_FRAME_TOPIC.to_owned(),
            },
        }]
    }

    fn detail(&self) -> FaultDetail<Value> {
        FaultDetail {
            environment_data: EnvironmentData {
                extended_data_records: ExtendedDataRecords {
                    first_occurrence: iso8601(self.first_occurred),
                    last_occurrence: iso8601(self.last_occurred),
                },
                snapshots: self.freeze_frames(),
            },
            item: FaultItem {
                code: self.code.to_owned(),
                fault_name: self.description.clone(),
                severity: self.severity.wire_value(),
                status: self.status_bits(),
            },
            x_medkit: FaultDetailMeta {
                occurrence_count: self.occurrence_count,
                reporting_sources: Vec::new(),
                severity_label: format!("{:?}", self.severity).to_uppercase(),
                status_raw: self.status_raw().to_owned(),
            },
        }
    }
}

/// Mutable diagnostic state, written by health-event ingestion and read by the
/// [`Provider`] seam.
#[derive(Debug)]
struct State {
    current: ConnectorHealthKind,
    last_sample: Option<Value>,
    dtcs: BTreeMap<&'static str, DtcRecord>,
}

impl Default for State {
    // A freshly-constructed binding has observed no transition yet and reports
    // the connector as nominally `Up`.
    fn default() -> Self {
        Self {
            current: ConnectorHealthKind::Up,
            last_sample: None,
            dtcs: BTreeMap::new(),
        }
    }
}

impl State {
    fn raise(
        &mut self,
        code: &'static str,
        severity: Severity,
        description: String,
        reason: String,
        at: f64,
    ) {
        let sample = self.capture_sample(severity, &reason);
        let entry = self
            .dtcs
            .entry(code)
            .or_insert_with(|| DtcRecord::new(code, severity));
        entry.description = description;
        entry.reason = reason;
        entry.last_occurred = at;
        if !entry.active {
            // A fresh raise (or a re-raise after healing) increments the
            // occurrence count and re-captures the freeze-frame (`REQ_0927`,
            // `REQ_0928`).
            entry.occurrence_count = entry.occurrence_count.saturating_add(1);
            if entry.occurrence_count == 1 {
                entry.first_occurred = at;
            }
            entry.active = true;
            entry.confirmed = true;
            entry.freeze_frame = sample;
            entry.captured_at = at;
        }
    }

    /// Clear a single DTC's active bit if present (UDS memory keeps the record).
    fn heal(&mut self, code: &str) {
        if let Some(entry) = self.dtcs.get_mut(code) {
            entry.active = false;
        }
    }

    /// Clear every active DTC (the connector returned to `Up`).
    fn heal_all(&mut self) {
        for entry in self.dtcs.values_mut() {
            entry.active = false;
        }
    }

    /// The last hook sample, or a synthesized snapshot of the health condition
    /// when no sample was observed (`REQ_0928`).
    fn capture_sample(&self, severity: Severity, reason: &str) -> Value {
        self.last_sample.clone().unwrap_or_else(|| {
            json!({
                "reason": reason,
                "severity": format!("{severity:?}").to_uppercase(),
            })
        })
    }

    fn worst_active_health(&self) -> Health {
        self.dtcs
            .values()
            .filter(|d| d.active)
            .map(|d| severity_to_health(d.severity))
            .max()
            .unwrap_or(Health::Ok)
    }
}

/// Binds a connector's [`ConnectorHealth`] transition stream into the medkit
/// [`Provider`] seam, producing a Component plus its DTCs.
///
/// Cloning shares the underlying store (`Arc`), so an ingestion handle and the
/// gateway's reading [`Provider`] can be separate clones of the same binding.
#[derive(Clone, Debug)]
pub struct MedkitProvider {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    component_id: String,
    name: String,
    description: Option<String>,
    state: Mutex<State>,
}

impl MedkitProvider {
    /// Create a binding for the connector represented by the SOVD Component
    /// `component_id` (e.g. `component:ethercat0`), with display `name`.
    ///
    /// The Component is emitted raw (no parent) so the manifest can place it
    /// when present; it works flat without one (`REQ_0926`).
    #[must_use]
    pub fn new(component_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                component_id: component_id.into(),
                name: name.into(),
                description: None,
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// Attach a human-readable description to the Component (e.g. the bus name).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        // `Arc::get_mut` succeeds because the builder owns the sole reference
        // before any clone is handed out.
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.description = Some(description.into());
        }
        self
    }

    /// The SOVD Component id this binding reports under.
    #[must_use]
    pub fn component_id(&self) -> &str {
        &self.inner.component_id
    }

    /// Record the last connector hook sample, captured into the freeze-frame at
    /// the next DTC confirmation (`REQ_0928`).
    pub fn observe_sample(&self, sample: Value) {
        self.lock().last_sample = Some(sample);
    }

    /// Ingest one connector health transition, updating the DTC store
    /// (`REQ_0926`, `REQ_0927`).
    ///
    /// `at_epoch_secs` is the wall-clock time (fractional epoch seconds) the
    /// off-path drain observed the event; the connector surface stamps
    /// transitions with a monotonic `Instant`, which cannot express the
    /// wall-clock occurrence / capture timestamps the contract carries.
    pub fn on_health_event(&self, event: &HealthEvent, at_epoch_secs: f64) {
        self.apply(&event.to, at_epoch_secs);
    }

    /// Apply a target health state directly (without constructing a
    /// [`HealthEvent`]); convenient for driving a simulated transition sequence.
    #[allow(
        clippy::significant_drop_tightening,
        reason = "the transition mutates the store atomically under one lock"
    )]
    pub fn apply(&self, to: &ConnectorHealth, at_epoch_secs: f64) {
        let mut state = self.lock();
        state.current = to.kind();
        match to {
            ConnectorHealth::Up => state.heal_all(),
            // Reconnecting: the fault condition persists until `Up` confirms
            // recovery, so active DTCs stay raised.
            ConnectorHealth::Connecting { .. } => {}
            ConnectorHealth::Degraded { reason } => {
                // Degraded means the stack is connected again: a prior
                // not-operational condition is superseded.
                state.heal(DTC_NOT_OPERATIONAL);
                state.raise(
                    DTC_DEGRADED,
                    Severity::Warn,
                    format!("Connector degraded: {reason}"),
                    reason.clone(),
                    at_epoch_secs,
                );
            }
            ConnectorHealth::Down { reason, .. } => {
                state.heal(DTC_DEGRADED);
                state.raise(
                    DTC_NOT_OPERATIONAL,
                    Severity::Critical,
                    format!("Fieldbus not operational: {reason}"),
                    reason.clone(),
                    at_epoch_secs,
                );
            }
        }
    }

    /// The full DTC detail for `fault_code`, with the last-sample freeze-frame
    /// under the contract's `snapshots` / `extended_data_records` shape
    /// (`REQ_0928`). `None` if no such DTC is in memory.
    #[must_use]
    pub fn fault_detail(&self, fault_code: &str) -> Option<FaultDetail<Value>> {
        self.lock().dtcs.get(fault_code).map(DtcRecord::detail)
    }

    /// The Component's currently-reported health (`REQ_0912`, `REQ_0926`).
    #[must_use]
    pub fn current_health(&self) -> Health {
        let (current, worst) = {
            let state = self.lock();
            (state.current, state.worst_active_health())
        };
        map_health(current).max(worst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The Component entity standing in for this connector.
    fn component(&self) -> Entity {
        Entity {
            href: format!("/api/v1/components/{}", self.inner.component_id),
            id: self.inner.component_id.clone(),
            name: self.inner.name.clone(),
            kind: EntityKind::Component,
            parent_id: None,
            description: self.inner.description.clone(),
            x_medkit: Some(EntityMeta {
                source: Some("runtime".to_owned()),
                ..EntityMeta::default()
            }),
        }
    }
}

/// The readable data tree served under the Component's `…/data`: the current
/// connector state plus, per DTC, its freeze-frame environment data — so the
/// confirmed freeze-frame is reachable through the running gateway (`REQ_0928`).
fn data_tree(state: &State) -> Value {
    let dtcs: serde_json::Map<String, Value> = state
        .dtcs
        .values()
        .map(|d| {
            (
                d.code.to_owned(),
                serde_json::to_value(d.detail().environment_data).unwrap_or(Value::Null),
            )
        })
        .collect();
    json!({
        "connector_health": {
            "state": format!("{:?}", state.current),
            "reason": state
                .dtcs
                .values()
                .filter(|d| d.active)
                .map(|d| d.reason.clone())
                .next(),
        },
        "dtcs": dtcs,
    })
}

impl Provider for MedkitProvider {
    fn entities(&self) -> Vec<Entity> {
        vec![self.component()]
    }

    fn faults(&self, entity_id: &str) -> Vec<FaultSummary> {
        if entity_id != self.inner.component_id {
            return Vec::new();
        }
        self.lock().dtcs.values().map(DtcRecord::summary).collect()
    }

    fn health(&self, entity_id: &str) -> Health {
        if entity_id != self.inner.component_id {
            return Health::Ok;
        }
        self.current_health()
    }

    #[allow(
        clippy::significant_drop_tightening,
        reason = "the store is read once under one lock for a consistent snapshot"
    )]
    fn snapshot(&self) -> ProviderSnapshot {
        // Read the store once under a single lock for a consistent snapshot,
        // then release it before assembling the wire model.
        let (summaries, environments, data_tree) = {
            let state = self.lock();
            let summaries: Vec<FaultSummary> =
                state.dtcs.values().map(DtcRecord::summary).collect();
            // Per-DTC freeze-frame environment data, carried through the snapshot
            // seam so the gateway's `…/faults/{code}` detail surfaces the real
            // `snapshots` / `extended_data_records` (`ADR_0116`, `REQ_0929`).
            let environments: BTreeMap<String, EnvironmentData<Value>> = state
                .dtcs
                .values()
                .map(|d| (d.code.to_owned(), d.detail().environment_data))
                .collect();
            (summaries, environments, data_tree(&state))
        };
        let mut faults = BTreeMap::new();
        if !summaries.is_empty() {
            faults.insert(self.inner.component_id.clone(), summaries);
        }
        let mut fault_environments = BTreeMap::new();
        if !environments.is_empty() {
            fault_environments.insert(self.inner.component_id.clone(), environments);
        }
        let mut data = BTreeMap::new();
        data.insert(self.inner.component_id.clone(), data_tree);
        ProviderSnapshot {
            entities: vec![self.component()],
            relationships: Vec::new(),
            faults,
            fault_environments,
            data,
            ..ProviderSnapshot::default()
        }
    }
}

fn bool01(value: bool) -> String {
    if value { "1" } else { "0" }.to_owned()
}

/// Format fractional epoch seconds as an ISO-8601 UTC timestamp with
/// millisecond precision (e.g. `2026-06-28T15:45:00.750Z`).
///
/// Uses Howard Hinnant's `civil_from_days` algorithm so the binding carries no
/// date-library dependency.
#[allow(
    clippy::cast_possible_truncation,
    reason = "epoch seconds are far inside the i64 millisecond range; round() before cast"
)]
fn iso8601(epoch_seconds: f64) -> String {
    let total_millis = (epoch_seconds * 1000.0).round() as i64;
    let mut secs = total_millis.div_euclid(1000);
    let millis = total_millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    secs = secs.rem_euclid(86_400);
    let (hour, minute, second) = (secs / 3600, (secs % 3600) / 60, secs % 60);

    // civil_from_days: days since 1970-01-01 -> (year, month, day).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn down(reason: &str) -> ConnectorHealth {
        ConnectorHealth::Down {
            reason: reason.to_owned(),
            since: Instant::now(),
        }
    }

    fn degraded(reason: &str) -> ConnectorHealth {
        ConnectorHealth::Degraded {
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn map_health_follows_worst_wins_ladder() {
        assert_eq!(map_health(ConnectorHealthKind::Up), Health::Ok);
        assert_eq!(map_health(ConnectorHealthKind::Connecting), Health::Warning);
        assert_eq!(map_health(ConnectorHealthKind::Degraded), Health::Warning);
        assert_eq!(map_health(ConnectorHealthKind::Down), Health::Error);
    }

    #[test]
    fn down_raises_critical_dtc_on_component() {
        let p = MedkitProvider::new("component:ethercat0", "EtherCAT bus 0");
        p.apply(&down("WKC mismatch"), 100.0);

        assert_eq!(p.health("component:ethercat0"), Health::Critical);
        let faults = p.faults("component:ethercat0");
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].fault_code, DTC_NOT_OPERATIONAL);
        assert_eq!(faults[0].severity, Severity::Critical.wire_value());
        assert_eq!(faults[0].status, "CONFIRMED");
    }

    #[test]
    fn degraded_raises_warning_dtc_carrying_reason() {
        let p = MedkitProvider::new("component:ethercat0", "EtherCAT bus 0");
        p.apply(&degraded("PUBACK timeout"), 5.0);

        assert_eq!(p.health("component:ethercat0"), Health::Warning);
        let detail = p.fault_detail(DTC_DEGRADED).expect("degraded DTC");
        assert!(detail.item.fault_name.contains("PUBACK timeout"));
        assert_eq!(detail.item.status.test_failed, "1");
        assert_eq!(detail.item.status.confirmed_dtc, "1");
    }

    #[test]
    fn up_heals_active_dtcs_but_keeps_memory() {
        let p = MedkitProvider::new("component:ethercat0", "EtherCAT bus 0");
        p.apply(&down("link loss"), 1.0);
        p.apply(&ConnectorHealth::Up, 2.0);

        assert_eq!(p.health("component:ethercat0"), Health::Ok);
        let detail = p.fault_detail(DTC_NOT_OPERATIONAL).expect("DTC in memory");
        assert_eq!(detail.item.status.test_failed, "0");
        // Confirmed bit stays latched after healing (UDS DTC memory).
        assert_eq!(detail.item.status.confirmed_dtc, "1");
        assert_eq!(detail.x_medkit.status_raw, "HEALED");
    }

    #[test]
    fn repeated_degraded_increments_occurrence_count() {
        let p = MedkitProvider::new("component:ethercat0", "EtherCAT bus 0");
        p.apply(&degraded("first"), 1.0);
        p.apply(&ConnectorHealth::Up, 2.0);
        p.apply(&degraded("second"), 3.0);

        let detail = p.fault_detail(DTC_DEGRADED).expect("degraded DTC");
        assert_eq!(detail.x_medkit.occurrence_count, 2);
        assert_eq!(
            detail
                .environment_data
                .extended_data_records
                .first_occurrence,
            iso8601(1.0)
        );
        assert_eq!(
            detail
                .environment_data
                .extended_data_records
                .last_occurrence,
            iso8601(3.0)
        );
    }

    #[test]
    fn confirmed_dtc_carries_last_sample_freeze_frame() {
        let p = MedkitProvider::new("component:ethercat0", "EtherCAT bus 0");
        p.observe_sample(json!({ "wkc": 2, "expected_wkc": 3 }));
        p.apply(&down("WKC mismatch"), 42.0);

        let detail = p.fault_detail(DTC_NOT_OPERATIONAL).expect("DTC");
        let frame = detail
            .environment_data
            .snapshots
            .first()
            .expect("freeze-frame present");
        assert_eq!(frame.kind, "freeze_frame");
        assert_eq!(frame.x_medkit.full_data["wkc"], json!(2));
        assert_eq!(frame.x_medkit.captured_at, iso8601(42.0));
    }

    #[test]
    fn synthesizes_freeze_frame_without_a_sample() {
        let p = MedkitProvider::new("component:can0", "CAN bus 0");
        p.apply(&degraded("bus-off"), 7.0);

        let detail = p.fault_detail(DTC_DEGRADED).expect("DTC");
        let frame = &detail.environment_data.snapshots[0];
        assert_eq!(frame.x_medkit.full_data["reason"], json!("bus-off"));
    }

    #[test]
    fn iso8601_matches_known_instant() {
        assert_eq!(iso8601(1_782_667_500.25), "2026-06-28T17:25:00.250Z");
    }
}
