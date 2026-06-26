//! Per-field `PropertyChanged` diffing and per-ViewModel staleness (REQ_0864,
//! REQ_0880).
//!
//! A ViewModel arrives on the wire as a JSON object (the server serializes the
//! struct's image to a `{ field: value, ... }` map — there is **no** per-field
//! metadata on the wire). The client therefore reconstructs per-field change
//! notifications by **diffing** each received object against the last held copy:
//! only fields whose [`serde_json::Value`] actually changed raise a
//! [`PropertyChange`] (REQ_0864). Because the payload is dynamic JSON, a received
//! ViewModel is represented as a [`serde_json::Map<String, Value>`] rather than a
//! statically-typed struct — this is what lets a read-only client (REQ_0876)
//! still display fields it matches by name.
//!
//! Staleness (REQ_0880) is tracked per ViewModel from the envelope
//! `sequence_number` / `timestamp_ns` plus a client-side receive instant, so a
//! frozen or never-seen ViewModel is distinguishable from a fresh one.

use std::time::{Duration, Instant};

use serde_json::{Map, Value};

/// One field that changed between two successive ViewModel values.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyChange {
    /// The field name (the JSON object key).
    pub field: String,
    /// The new value.
    pub value: Value,
}

/// Diff a freshly-received ViewModel object against the previously-held copy,
/// returning one [`PropertyChange`] per field whose value actually changed
/// (REQ_0864).
///
/// A field present in `next` but absent in `prev` (e.g. the very first value, or
/// a field a read-only client did not previously hold) counts as changed. Fields
/// present in `prev` but absent in `next` are ignored — a POD ViewModel has a
/// fixed field set, so this only arises across an incompatible (read-only)
/// binding, where surfacing a spurious "removed" notification would be noise.
///
/// The result is in `next`'s key order (`serde_json::Map` is ordered), so the
/// notification order is deterministic.
#[must_use]
pub fn diff_fields(prev: &Map<String, Value>, next: &Map<String, Value>) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    for (field, value) in next {
        if prev.get(field) != Some(value) {
            changes.push(PropertyChange {
                field: field.clone(),
                value: value.clone(),
            });
        }
    }
    changes
}

/// The freshness of a ViewModel relative to a staleness `threshold` (REQ_0880).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staleness {
    /// No value has ever been received for this ViewModel.
    NeverReceived,
    /// A value was received within `threshold` (the ViewModel is current).
    Fresh {
        /// Time since the last received value.
        age: Duration,
    },
    /// The last received value is older than `threshold` (the ViewModel is
    /// frozen / its producer appears stalled).
    Stale {
        /// Time since the last received value.
        age: Duration,
    },
}

impl Staleness {
    /// Whether this ViewModel is stale or was never received (i.e. not fresh).
    #[must_use]
    pub const fn is_stale(self) -> bool {
        !matches!(self, Staleness::Fresh { .. })
    }
}

/// Compute [`Staleness`] from the instant the last value was received (`None`
/// when nothing has arrived yet), the current instant, and the `threshold`.
///
/// Pure and total: `last_received == None` → [`Staleness::NeverReceived`];
/// otherwise the age is `now - last_received` and the result is
/// [`Staleness::Fresh`] iff that age is `<= threshold`.
#[must_use]
pub fn staleness(last_received: Option<Instant>, now: Instant, threshold: Duration) -> Staleness {
    match last_received {
        None => Staleness::NeverReceived,
        Some(t) => {
            let age = now.saturating_duration_since(t);
            if age <= threshold {
                Staleness::Fresh { age }
            } else {
                Staleness::Stale { age }
            }
        }
    }
}

/// The last-held copy of one ViewModel plus its receive bookkeeping.
///
/// Holds the most-recent field map and the envelope `sequence_number` /
/// `timestamp_ns` and client-side receive [`Instant`] of the last accepted
/// value, so [`ViewModelState::observe`] can both diff per field (REQ_0864) and
/// report staleness (REQ_0880).
#[derive(Clone, Debug, Default)]
pub struct ViewModelState {
    fields: Map<String, Value>,
    last_sequence: Option<u64>,
    last_timestamp_ns: Option<u64>,
    last_received: Option<Instant>,
}

impl ViewModelState {
    /// A fresh state that has observed nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The current field map (the last accepted value), empty until the first
    /// [`observe`](Self::observe).
    #[must_use]
    pub fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    /// The `sequence_number` of the last accepted envelope, if any.
    #[must_use]
    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// The sender `timestamp_ns` of the last accepted envelope, if any.
    #[must_use]
    pub fn last_timestamp_ns(&self) -> Option<u64> {
        self.last_timestamp_ns
    }

    /// The staleness of this ViewModel as of `now`, against `threshold`.
    #[must_use]
    pub fn staleness(&self, now: Instant, threshold: Duration) -> Staleness {
        staleness(self.last_received, now, threshold)
    }

    /// Observe a newly-received ViewModel value (field map + envelope metadata)
    /// at receive instant `received_at`, returning the per-field changes.
    ///
    /// Updates the held copy and the receive bookkeeping, then returns the diff
    /// vs the previous copy (REQ_0864). The first observation reports every field
    /// as changed.
    pub fn observe(
        &mut self,
        next: Map<String, Value>,
        sequence_number: u64,
        timestamp_ns: u64,
        received_at: Instant,
    ) -> Vec<PropertyChange> {
        let changes = diff_fields(&self.fields, &next);
        self.fields = next;
        self.last_sequence = Some(sequence_number);
        self.last_timestamp_ns = Some(timestamp_ns);
        self.last_received = Some(received_at);
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn first_value_reports_every_field_changed() {
        let prev = Map::new();
        let next = map(&[("position", json!(1.0)), ("state", json!("idle"))]);
        let changes = diff_fields(&prev, &next);
        assert_eq!(changes.len(), 2);
        // Ordered by key (serde_json::Map is ordered).
        assert_eq!(changes[0].field, "position");
        assert_eq!(changes[1].field, "state");
    }

    #[test]
    fn only_changed_fields_are_reported() {
        let prev = map(&[("position", json!(1.0)), ("state", json!("idle"))]);
        let next = map(&[("position", json!(2.0)), ("state", json!("idle"))]);
        let changes = diff_fields(&prev, &next);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "position");
        assert_eq!(changes[0].value, json!(2.0));
    }

    #[test]
    fn identical_values_report_no_changes() {
        let prev = map(&[("position", json!(1.0))]);
        let next = map(&[("position", json!(1.0))]);
        assert!(diff_fields(&prev, &next).is_empty());
    }

    #[test]
    fn never_received_is_distinct_from_fresh() {
        let now = Instant::now();
        assert_eq!(
            staleness(None, now, Duration::from_millis(100)),
            Staleness::NeverReceived
        );
    }

    #[test]
    fn within_threshold_is_fresh_beyond_is_stale() {
        let now = Instant::now();
        let recent = now - Duration::from_millis(50);
        let old = now - Duration::from_millis(500);
        let threshold = Duration::from_millis(100);
        assert!(matches!(
            staleness(Some(recent), now, threshold),
            Staleness::Fresh { .. }
        ));
        assert!(matches!(
            staleness(Some(old), now, threshold),
            Staleness::Stale { .. }
        ));
    }

    #[test]
    fn state_observe_diffs_and_tracks_metadata() {
        let mut state = ViewModelState::new();
        let t0 = Instant::now();
        let first = state.observe(map(&[("position", json!(1.0))]), 10, 1000, t0);
        assert_eq!(first.len(), 1);
        assert_eq!(state.last_sequence(), Some(10));
        assert_eq!(state.last_timestamp_ns(), Some(1000));

        let t1 = t0 + Duration::from_millis(10);
        let second = state.observe(map(&[("position", json!(1.0))]), 11, 1100, t1);
        assert!(
            second.is_empty(),
            "unchanged value raises no PropertyChanged"
        );
        assert_eq!(state.last_sequence(), Some(11));
    }

    #[test]
    fn state_staleness_uses_last_receive_instant() {
        let mut state = ViewModelState::new();
        assert_eq!(
            state.staleness(Instant::now(), Duration::from_millis(100)),
            Staleness::NeverReceived
        );
        let t0 = Instant::now();
        state.observe(Map::new(), 1, 1, t0);
        let later = t0 + Duration::from_secs(10);
        assert!(
            state
                .staleness(later, Duration::from_millis(100))
                .is_stale()
        );
    }
}
