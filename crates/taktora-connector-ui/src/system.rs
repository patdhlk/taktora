//! [`SystemViewModel`]: the mandatory liveness heartbeat (`REQ_0879`).
//!
//! The connector **always** publishes a `SystemViewModel` carrying a monotonic
//! `counter` that advances every pump tick and a process `epoch` that uniquely
//! identifies this application instance. It is the canonical "application alive
//! and pump running" signal — distinguishable from a static-but-live ViewModel —
//! and is exempt from the zero-subscriber skip (`REQ_0862`) so a UI can always
//! attach and detect liveness.
//!
//! # Epoch derivation (restart- and instance-distinct)
//!
//! `REQ_0879` requires the epoch to identify *this* process instance, and
//! `REQ_0882` requires an application restart to bump it. A bare
//! [`std::process::id`] is **not** restart-distinct: pids are recycled, and a
//! containerised application is frequently pid 1 every run. [`default_epoch`]
//! therefore mixes the wall-clock nanosecond reading at first observation with
//! the pid: the time term makes it distinct across restarts (a fresh start
//! reads a later clock), and XOR-ing the pid keeps two applications that launch
//! in the same nanosecond distinct. Wall-clock is the correct source here —
//! this is runtime code reporting liveness, not a build-reproducibility
//! constraint. The value is stable within one process (a `OnceLock` caches the
//! first observation). A caller may override it via [`system_entry`]'s `epoch`
//! argument.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use taktora_connector_ui_contract::{FieldSchema, FieldType, ViewModelSchema};

use crate::pump::{EncodeFn, PumpEntry, VmPublisher};

/// The logical name of the heartbeat ViewModel (and its manifest entry).
pub const SYSTEM_VIEW_MODEL_NAME: &str = "System";

/// The mandatory liveness heartbeat ViewModel (`REQ_0879`).
///
/// Both fields are `u64`, so this is a plain POD struct; it is hand-described
/// via [`SystemViewModel::schema`] rather than `#[derive(ViewModel)]` because
/// the derive targets `::taktora_connector_ui` and cannot run inside this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SystemViewModel {
    /// Monotonic counter, advanced once per pump tick.
    pub counter: u64,
    /// Process-unique epoch identifying this application instance.
    pub epoch: u64,
}

impl SystemViewModel {
    /// The manifest schema contribution for the heartbeat.
    #[must_use]
    pub fn schema() -> ViewModelSchema {
        ViewModelSchema {
            name: SYSTEM_VIEW_MODEL_NAME.to_owned(),
            service: String::new(),
            fields: vec![
                FieldSchema {
                    name: "counter".to_owned(),
                    ty: FieldType::U64,
                },
                FieldSchema {
                    name: "epoch".to_owned(),
                    ty: FieldType::U64,
                },
            ],
        }
    }
}

/// The process epoch: the first-observation wall-clock nanosecond reading
/// XOR-ed with [`std::process::id`], cached so it is stable within the process.
///
/// Mixing both sources makes the epoch **restart-distinct** (the time term
/// advances every launch, unlike a recycled or container-pinned pid) and
/// **instance-distinct** (the pid term separates two applications that start in
/// the same nanosecond), satisfying `REQ_0879`/`REQ_0882`. See the module docs.
#[must_use]
pub fn default_epoch() -> u64 {
    static EPOCH: OnceLock<u64> = OnceLock::new();
    *EPOCH.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        nanos ^ (std::process::id() as u64)
    })
}

/// Build the exempt pump entry that publishes the [`SystemViewModel`] heartbeat.
///
/// The returned entry is exempt from the zero-subscriber skip and advances its
/// `counter` every tick, always reporting a change so it publishes each tick.
#[must_use]
pub fn system_entry<P>(epoch: u64, publisher: P) -> PumpEntry
where
    P: VmPublisher + 'static,
{
    let mut counter: u64 = 0;
    let encode: EncodeFn = Box::new(move |out: &mut Vec<u8>| {
        let vm = SystemViewModel { counter, epoch };
        counter = counter.wrapping_add(1);
        out.clear();
        serde_json::to_writer(&mut *out, &vm).ok()?;
        Some(true)
    });
    PumpEntry::new(SYSTEM_VIEW_MODEL_NAME, true, encode, Box::new(publisher))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump::{MockPublisher, Pump};

    fn parse(bytes: &[u8]) -> SystemViewModel {
        // SystemViewModel is serialize-only (it is only ever published); parse
        // the wire JSON via a generic Value in tests rather than widening the
        // type with a Deserialize derive it does not otherwise need.
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        SystemViewModel {
            counter: v["counter"].as_u64().unwrap(),
            epoch: v["epoch"].as_u64().unwrap(),
        }
    }

    #[test]
    fn schema_describes_counter_and_epoch() {
        let s = SystemViewModel::schema();
        assert_eq!(s.name, "System");
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0].name, "counter");
        assert_eq!(s.fields[1].name, "epoch");
    }

    #[test]
    fn epoch_is_stable_within_the_process() {
        assert_eq!(default_epoch(), default_epoch());
    }

    #[test]
    fn epoch_mixes_time_and_pid_so_it_is_not_bare_pid() {
        // Restart-distinctness comes from the wall-clock term, instance-
        // distinctness from the pid term. The defining property we can assert
        // without sleeps is that the epoch is *not* the bare pid (the time term
        // contributed high bits), so PID recycling / pid-1 containers cannot
        // collapse it to a non-restart-distinct value.
        let pid = u64::from(std::process::id());
        assert_ne!(
            default_epoch(),
            pid,
            "epoch must incorporate wall-clock, not just the pid"
        );
    }

    #[test]
    fn counter_advances_each_tick() {
        let mock = MockPublisher::new(); // zero subscribers on purpose
        let mut pump = Pump::new();
        pump.add_entry(system_entry(7, mock.clone()));

        pump.tick();
        pump.tick();
        pump.tick();

        let published = mock.published();
        assert_eq!(published.len(), 3, "heartbeat must publish every tick");
        let counters: Vec<u64> = published.iter().map(|b| parse(b).counter).collect();
        assert_eq!(counters, vec![0, 1, 2]);
        // Epoch is constant across ticks.
        for b in &published {
            assert_eq!(parse(b).epoch, 7);
        }
    }

    #[test]
    fn heartbeat_is_exempt_from_zero_subscriber_skip() {
        let mock = MockPublisher::new(); // zero subscribers
        let mut pump = Pump::new();
        pump.add_entry(system_entry(default_epoch(), mock.clone()));

        let stats = pump.tick();
        assert_eq!(stats.published, 1);
        assert_eq!(stats.skipped_zero_sub, 0);
    }
}
