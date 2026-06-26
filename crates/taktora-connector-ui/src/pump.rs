//! The non-RT publisher pump (`REQ_0861`, `REQ_0862`).
//!
//! The pump runs on its **own** OS thread (never the executor's WaitSet thread,
//! mirroring [`taktora_telemetry_export`]'s drain thread). Each tick it walks
//! every registered entry, snapshots its [`PropertyReader`] off the RT path,
//! JSON-encodes the reconstructed ViewModel, and publishes **one** envelope —
//! but only when the snapshot changed since the last publish (intermediate
//! values are coalesced to the latest, lossy by design for state properties).
//!
//! [`PropertyReader`]: crate::PropertyReader
//! [`taktora_telemetry_export`]: https://docs.rs/taktora-telemetry-export
//!
//! # The publisher seam
//!
//! The pump never depends on iceoryx2 directly: it talks to a [`VmPublisher`]
//! trait. Production wires an [`IoxVmPublisher`](crate::IoxVmPublisher); unit
//! tests wire a [`MockPublisher`] that records payloads and exposes a settable
//! subscriber count, so coalescing, the zero-subscriber skip (`REQ_0862`), and
//! the manifest / `SystemViewModel` exemptions are all testable deterministically
//! with no shared memory and no sleeps (drive [`Pump::tick`] by hand).
//!
//! # Heterogeneous entries
//!
//! ViewModels are different Rust types, so an entry stores a **type-erased**
//! encode closure ([`EncodeFn`]) — it snapshots its own reader, encodes into a
//! caller buffer, and reports whether the value changed — paired with its boxed
//! [`VmPublisher`]. [`property_entry`] builds the closure for a typed
//! [`PropertyReader`]; the manifest and heartbeat build their own.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use taktora_connector_core::ConnectorError;

use crate::property::PropertyReader;
use crate::viewmodel::ViewModel;

/// A destination the pump publishes one ViewModel envelope to.
///
/// This is the testability seam: the pump calls only [`publish`](Self::publish)
/// and [`subscriber_count`](Self::subscriber_count), so it never hard-depends on
/// iceoryx2. The production impl is [`IoxVmPublisher`](crate::IoxVmPublisher);
/// [`MockPublisher`] backs unit tests.
pub trait VmPublisher: Send {
    /// Publish `bytes` as one envelope (latest-value, history depth 1).
    ///
    /// # Errors
    ///
    /// Returns a [`ConnectorError`] — notably [`ConnectorError::BackPressure`]
    /// when the outbound channel is saturated — which the pump surfaces in its
    /// [`PumpTickStats`] so a health monitor can react (`REQ_0883`).
    fn publish(&self, bytes: &[u8]) -> Result<(), ConnectorError>;

    /// The number of subscribers currently attached to this service.
    ///
    /// The pump skips encoding and publishing a ViewModel with zero subscribers
    /// (`REQ_0862`), except entries marked exempt (the manifest and the
    /// `SystemViewModel` heartbeat).
    fn subscriber_count(&self) -> usize;
}

/// A type-erased "snapshot, encode, and report change" step for one entry.
///
/// Clears `buf`, writes the entry's current JSON encoding into it, and returns:
/// `None` if the entry has no value yet (never set); `Some(true)` if the value
/// changed since the previous call; `Some(false)` if it is unchanged. When
/// `Some` is returned `buf` always holds the current encoding (so an exempt or
/// newly-subscribed entry can be published even when unchanged).
pub type EncodeFn = Box<dyn FnMut(&mut Vec<u8>) -> Option<bool> + Send>;

/// One pump entry: a named, type-erased ViewModel encode step plus the
/// [`VmPublisher`] it publishes to.
pub struct PumpEntry {
    name: String,
    exempt: bool,
    encode: EncodeFn,
    publisher: Box<dyn VmPublisher>,
    had_subscribers: bool,
}

impl PumpEntry {
    /// Assemble an entry from its parts.
    ///
    /// `exempt` entries (the manifest service per `REQ_0872` and the
    /// `SystemViewModel` heartbeat per `REQ_0879`) bypass the zero-subscriber
    /// skip and are published every tick so a UI can always attach and detect
    /// liveness.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        exempt: bool,
        encode: EncodeFn,
        publisher: Box<dyn VmPublisher>,
    ) -> Self {
        Self {
            name: name.into(),
            exempt,
            encode,
            publisher,
            had_subscribers: false,
        }
    }

    /// The entry's logical name (used in trace logs).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Build a pump entry for a typed [`PropertyReader`].
///
/// The returned entry snapshots the reader off the RT path each tick, encodes
/// the reconstructed ViewModel to JSON, and coalesces unchanged values. It is
/// **not** exempt, so it is skipped while it has zero subscribers (`REQ_0862`).
#[must_use]
pub fn property_entry<V, P>(
    name: impl Into<String>,
    reader: PropertyReader<V>,
    publisher: P,
) -> PumpEntry
where
    V: ViewModel + Serialize + 'static,
    P: VmPublisher + 'static,
{
    PumpEntry::new(name, false, encode_property(reader), Box::new(publisher))
}

/// The [`EncodeFn`] for a typed property: snapshot, JSON-encode, diff.
fn encode_property<V>(reader: PropertyReader<V>) -> EncodeFn
where
    V: ViewModel + Serialize + 'static,
{
    // Per-entry scratch: the reusable image buffer the seqlock reader copies
    // into, and the last published JSON (the coalescing marker). Both are owned
    // by the closure, so each entry keeps its own state.
    let mut image_buf: Vec<u8> = Vec::new();
    let mut last_json: Vec<u8> = Vec::new();
    let mut have_last = false;
    Box::new(move |out: &mut Vec<u8>| -> Option<bool> {
        let vm = reader.snapshot_into(&mut image_buf)?;
        out.clear();
        // JSON encoding of a reconstructed POD ViewModel is infallible; if it
        // ever errs we treat the entry as having no value this tick rather than
        // panicking on the pump thread.
        serde_json::to_writer(&mut *out, &vm).ok()?;
        let changed = !have_last || *out != last_json;
        if changed {
            last_json.clear();
            last_json.extend_from_slice(out);
            have_last = true;
        }
        Some(changed)
    })
}

/// Per-tick counters returned by [`Pump::tick`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PumpTickStats {
    /// Entries published this tick.
    pub published: usize,
    /// Entries skipped because they had zero subscribers and were not exempt
    /// (`REQ_0862`).
    pub skipped_zero_sub: usize,
    /// Entries skipped because their value was unchanged since the last publish
    /// (coalescing).
    pub skipped_unchanged: usize,
    /// Entries skipped because they have no value yet (never set).
    pub no_value: usize,
    /// Entries whose publish call returned an error (e.g. back-pressure).
    pub publish_errors: usize,
}

/// The non-RT publisher pump.
///
/// Build it by [`add_entry`](Self::add_entry)ing entries, then either drive it
/// by hand with [`tick`](Self::tick) (tests) or hand it to its own OS thread
/// with [`spawn`](Self::spawn) (production).
#[derive(Default)]
pub struct Pump {
    entries: Vec<PumpEntry>,
    scratch: Vec<u8>,
}

impl Pump {
    /// An empty pump.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an entry. Returns `&mut self` for chaining.
    pub fn add_entry(&mut self, entry: PumpEntry) -> &mut Self {
        self.entries.push(entry);
        self
    }

    /// The number of registered entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the pump has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Run one pass over every entry, publishing the ones that should publish.
    ///
    /// Deterministic and synchronous — the unit tests call this directly so they
    /// need no timers or sleeps.
    pub fn tick(&mut self) -> PumpTickStats {
        let mut stats = PumpTickStats::default();
        for entry in &mut self.entries {
            let count = entry.publisher.subscriber_count();
            let active = entry.exempt || count > 0;
            if !active {
                entry.had_subscribers = false;
                stats.skipped_zero_sub += 1;
                continue;
            }
            // A non-exempt entry that just gained its first subscriber is
            // republished even if its value is unchanged, so the new subscriber
            // sees the current value on the next tick (`REQ_0862`).
            let reappeared = !entry.had_subscribers && count > 0;
            entry.had_subscribers = count > 0;
            let force = entry.exempt || reappeared;

            match (entry.encode)(&mut self.scratch) {
                None => stats.no_value += 1,
                Some(changed) => {
                    if changed || force {
                        match entry.publisher.publish(&self.scratch) {
                            Ok(()) => stats.published += 1,
                            Err(err) => {
                                stats.publish_errors += 1;
                                tracing::warn!(entry = %entry.name, error = %err, "ui pump publish failed");
                            }
                        }
                    } else {
                        stats.skipped_unchanged += 1;
                    }
                }
            }
        }
        stats
    }

    /// Spawn the pump on its own OS thread, ticking every `cadence`.
    ///
    /// The thread runs until [`PumpHandle::stop`] is called, then performs one
    /// final tick so the latest values are flushed before exit (mirroring
    /// `taktora-telemetry-export`'s final drain). Returns a [`PumpHandle`].
    pub fn spawn(mut self, cadence: Duration) -> PumpHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || -> u64 {
            let mut total_published: u64 = 0;
            loop {
                let stopping = stop_thread.load(Ordering::Acquire);
                let stats = self.tick();
                total_published += stats.published as u64;
                if stopping {
                    // This tick was the final drain.
                    break;
                }
                thread::sleep(cadence);
            }
            total_published
        });
        PumpHandle { stop, handle }
    }
}

/// Handle to a running pump thread. Call [`stop`](Self::stop) to flush and join.
#[must_use = "call stop() to flush and join the pump thread; dropping it leaks the thread"]
pub struct PumpHandle {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<u64>,
}

impl PumpHandle {
    /// Signal the pump to perform one final tick, then join it. Returns the
    /// total number of envelopes published over the thread's lifetime.
    ///
    /// # Panics
    ///
    /// Panics if the pump thread panicked.
    pub fn stop(self) -> u64 {
        self.stop.store(true, Ordering::Release);
        self.handle.join().expect("pump thread panicked")
    }
}

/// A test [`VmPublisher`] that records published payloads and exposes a settable
/// subscriber count and back-pressure switch.
///
/// `MockPublisher` is `Clone`: every clone shares one backing state, so a test
/// can keep one handle to inspect while moving a clone into the pump.
#[derive(Clone, Default)]
pub struct MockPublisher {
    inner: Arc<MockState>,
}

#[derive(Default)]
struct MockState {
    published: Mutex<Vec<Vec<u8>>>,
    subscribers: AtomicUsize,
    backpressure: AtomicBool,
}

impl MockPublisher {
    /// A publisher with zero subscribers and no back-pressure.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A publisher pre-set with `subscribers` attached.
    #[must_use]
    pub fn with_subscribers(subscribers: usize) -> Self {
        let p = Self::new();
        p.set_subscriber_count(subscribers);
        p
    }

    /// Set the reported subscriber count.
    pub fn set_subscriber_count(&self, n: usize) {
        self.inner.subscribers.store(n, Ordering::Relaxed);
    }

    /// Make subsequent [`VmPublisher::publish`] calls fail with
    /// [`ConnectorError::BackPressure`] (when `on`) or succeed (when `off`).
    pub fn set_backpressure(&self, on: bool) {
        self.inner.backpressure.store(on, Ordering::Relaxed);
    }

    /// Every payload published so far, in order.
    #[must_use]
    pub fn published(&self) -> Vec<Vec<u8>> {
        self.inner.published.lock().expect("mock lock").clone()
    }

    /// How many payloads have been published.
    #[must_use]
    pub fn publish_count(&self) -> usize {
        self.inner.published.lock().expect("mock lock").len()
    }

    /// The most recently published payload, if any.
    #[must_use]
    pub fn last_published(&self) -> Option<Vec<u8>> {
        self.inner
            .published
            .lock()
            .expect("mock lock")
            .last()
            .cloned()
    }
}

impl VmPublisher for MockPublisher {
    fn publish(&self, bytes: &[u8]) -> Result<(), ConnectorError> {
        if self.inner.backpressure.load(Ordering::Relaxed) {
            return Err(ConnectorError::BackPressure);
        }
        self.inner
            .published
            .lock()
            .expect("mock lock")
            .push(bytes.to_vec());
        Ok(())
    }

    fn subscriber_count(&self) -> usize {
        self.inner.subscribers.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::Property;
    use serde::Serialize;
    use taktora_connector_ui_contract::{FieldSchema, FieldType, ViewModelSchema};

    // A minimal hand-rolled ViewModel (the derive cannot run inside this crate;
    // it targets `::taktora_connector_ui`). One `f64` field is enough to
    // exercise the pump.
    #[derive(Clone, Copy, Debug, PartialEq, Serialize)]
    struct Scalar {
        v: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ScalarImage {
        v: f64,
    }

    impl ViewModel for Scalar {
        type Image = ScalarImage;
        const IMAGE_SIZE: usize = core::mem::size_of::<ScalarImage>();
        const MAX_ENCODED_SIZE: usize = 32;
        fn schema() -> ViewModelSchema {
            ViewModelSchema {
                name: "Scalar".into(),
                service: String::new(),
                fields: vec![FieldSchema {
                    name: "v".into(),
                    ty: FieldType::F64,
                }],
            }
        }
        fn to_image(&self) -> ScalarImage {
            ScalarImage { v: self.v }
        }
        fn from_image(image: &ScalarImage) -> Self {
            Self { v: image.v }
        }
        fn image_to_json(image: &ScalarImage, buf: &mut Vec<u8>) {
            let vm = Self::from_image(image);
            serde_json::to_writer(buf, &vm).expect("infallible");
        }
    }

    fn json_v(bytes: &[u8]) -> f64 {
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        value["v"].as_f64().unwrap()
    }

    #[test]
    fn coalesces_intermediate_values_to_the_latest() {
        let prop = Property::<Scalar>::new();
        let mock = MockPublisher::with_subscribers(1);
        let mut pump = Pump::new();
        pump.add_entry(property_entry("Scalar", prop.reader(), mock.clone()));

        // Three writes between ticks: only the latest (C) is published once.
        prop.set(&Scalar { v: 1.0 });
        prop.set(&Scalar { v: 2.0 });
        prop.set(&Scalar { v: 3.0 });
        let stats = pump.tick();

        assert_eq!(stats.published, 1);
        assert_eq!(mock.publish_count(), 1);
        assert_eq!(json_v(&mock.last_published().unwrap()), 3.0);
    }

    #[test]
    fn unchanged_value_is_not_republished() {
        let prop = Property::<Scalar>::new();
        let mock = MockPublisher::with_subscribers(1);
        let mut pump = Pump::new();
        pump.add_entry(property_entry("Scalar", prop.reader(), mock.clone()));

        prop.set(&Scalar { v: 7.0 });
        let first = pump.tick();
        assert_eq!(first.published, 1);

        // No new write: the second tick coalesces to "unchanged" and skips.
        let second = pump.tick();
        assert_eq!(second.published, 0);
        assert_eq!(second.skipped_unchanged, 1);
        assert_eq!(mock.publish_count(), 1);
    }

    #[test]
    fn zero_subscriber_view_model_is_skipped() {
        let prop = Property::<Scalar>::new();
        let mock = MockPublisher::new(); // zero subscribers
        let mut pump = Pump::new();
        pump.add_entry(property_entry("Scalar", prop.reader(), mock.clone()));

        prop.set(&Scalar { v: 5.0 });
        let stats = pump.tick();
        assert_eq!(stats.published, 0);
        assert_eq!(stats.skipped_zero_sub, 1);
        assert_eq!(mock.publish_count(), 0);

        // Once a subscriber attaches the pump resumes on the next tick.
        mock.set_subscriber_count(1);
        let stats = pump.tick();
        assert_eq!(stats.published, 1);
        assert_eq!(json_v(&mock.last_published().unwrap()), 5.0);
    }

    #[test]
    fn exempt_entry_publishes_with_zero_subscribers() {
        // An exempt entry (manifest / SystemViewModel) publishes even with no
        // subscribers, every tick.
        let mock = MockPublisher::new(); // zero subscribers
        let mut tick_count = 0u64;
        let encode: EncodeFn = Box::new(move |out: &mut Vec<u8>| {
            tick_count += 1;
            out.clear();
            out.extend_from_slice(format!("{{\"n\":{tick_count}}}").as_bytes());
            Some(true)
        });
        let mut pump = Pump::new();
        pump.add_entry(PumpEntry::new(
            "System",
            true,
            encode,
            Box::new(mock.clone()),
        ));

        let s1 = pump.tick();
        let s2 = pump.tick();
        assert_eq!(s1.published, 1);
        assert_eq!(s2.published, 1);
        assert_eq!(mock.publish_count(), 2);
        assert_eq!(s1.skipped_zero_sub, 0);
    }

    #[test]
    fn reappearing_subscriber_forces_a_republish_of_unchanged_value() {
        let prop = Property::<Scalar>::new();
        let mock = MockPublisher::with_subscribers(1);
        let mut pump = Pump::new();
        pump.add_entry(property_entry("Scalar", prop.reader(), mock.clone()));

        prop.set(&Scalar { v: 1.0 });
        assert_eq!(pump.tick().published, 1);

        // Subscriber leaves: skipped.
        mock.set_subscriber_count(0);
        assert_eq!(pump.tick().skipped_zero_sub, 1);

        // Subscriber returns, value unchanged: forced republish so the new
        // subscriber sees the current value.
        mock.set_subscriber_count(1);
        let stats = pump.tick();
        assert_eq!(stats.published, 1);
        assert_eq!(mock.publish_count(), 2);
    }

    #[test]
    fn publish_error_is_counted() {
        let prop = Property::<Scalar>::new();
        let mock = MockPublisher::with_subscribers(1);
        mock.set_backpressure(true);
        let mut pump = Pump::new();
        pump.add_entry(property_entry("Scalar", prop.reader(), mock.clone()));

        prop.set(&Scalar { v: 1.0 });
        let stats = pump.tick();
        assert_eq!(stats.publish_errors, 1);
        assert_eq!(stats.published, 0);
        assert_eq!(mock.publish_count(), 0);
    }

    #[test]
    fn spawn_then_stop_is_clean_and_flushes() {
        let prop = Property::<Scalar>::new();
        let mock = MockPublisher::with_subscribers(1);
        let mut pump = Pump::new();
        pump.add_entry(property_entry("Scalar", prop.reader(), mock.clone()));
        prop.set(&Scalar { v: 42.0 });

        let handle = pump.spawn(Duration::from_millis(2));
        // Give the thread a few ticks.
        thread::sleep(Duration::from_millis(30));
        let total = handle.stop();

        assert!(total >= 1, "pump published nothing");
        assert_eq!(json_v(&mock.last_published().unwrap()), 42.0);
        // After the first publish, coalescing means the value is not republished
        // every tick — exactly one payload for one unchanged value.
        assert_eq!(mock.publish_count(), 1);
    }
}
