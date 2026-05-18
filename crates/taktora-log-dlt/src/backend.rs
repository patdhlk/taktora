//! `DltBackend` — the public DLT [`LogSink`] and [`log::Log`] adapter.
//!
//! Wires together the encoder (T10), level table (T13), offline ring
//! (T14), control parser (T15), and background flusher (T16) into a
//! single object that callers install as the global logger.
//!
//! See `spec/requirements/logging.rst` (REQ_0806, REQ_0807, REQ_0810,
//! REQ_0811, REQ_0812, REQ_0814, REQ_0815) for the contract this type
//! is required to satisfy.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crossbeam_channel::{Sender, TrySendError};
use thiserror::Error;

use taktora_log::LogSink;

use crate::encode::Encoder;
use crate::flusher::{FlusherConfig, FlusherHandle, SummaryBuilder, spawn_flusher};
use crate::ids::{AppId, CtxId};
use crate::level_table::LevelTable;
use crate::ring::OfflineRing;
use crate::transport::TransportConfig;

/// Default offline-ring capacity (records) when the builder leaves it
/// unset. Sized to comfortably absorb a multi-second daemon outage at
/// realistic log rates without blowing application heap.
const DEFAULT_RING_CAPACITY: usize = 512;

/// Default initial reconnect backoff for the background flusher.
const DEFAULT_RECONNECT_INITIAL_BACKOFF: Duration = Duration::from_millis(50);

/// Default upper bound for the exponential reconnect backoff.
const DEFAULT_RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(2);

/// Errors returned by [`DltBackendBuilder::build`].
#[derive(Debug, Error)]
pub enum BuildError {
    /// A required builder field was not supplied. The inner string names
    /// the missing setter (`app`, `default_context`, `ecu_id`,
    /// `transport`).
    #[error("required field {0} is missing")]
    Missing(&'static str),
}

/// Public DLT backend handle.
///
/// Implements both [`LogSink`] (the `taktora-log` backend extension
/// trait) and [`log::Log`] (the standard `log` facade) so that callers
/// can either install it via `taktora-log`'s facade builder or pass it
/// directly to [`log::set_boxed_logger`].
///
/// All state is internally synchronised — `DltBackend` is `Send + Sync`
/// and is safe to share across threads via [`Arc`].
pub struct DltBackend {
    /// Shared encoder. Held behind [`Arc`] so future helpers (e.g. the
    /// T19 drop-summary record builder) can hold a cheap clone without
    /// taking ownership away from the backend itself.
    encoder: Arc<Encoder>,
    /// Producer side of the bounded channel feeding the flusher thread.
    tx: Sender<Vec<u8>>,
    /// Per-context level table — read by [`Self::enabled`] and updated
    /// by daemon control messages routed through the flusher.
    level_table: Arc<LevelTable>,
    /// Offline buffer used when the channel is full or the flusher is
    /// disconnected. Drained FIFO on daemon reconnect (REQ_0814).
    ring: Arc<OfflineRing>,
    /// Handle to the background flusher thread. Wrapped in a `Mutex<Option>`
    /// so [`Self::shutdown`] can move the handle out (consuming `shutdown`
    /// on `FlusherHandle`) without needing `&mut self`.
    flusher: std::sync::Mutex<Option<FlusherHandle>>,
    /// Monotonic per-backend counter feeding the DLT standard-header
    /// timestamp field. Wraps at `u32::MAX` — adequate for v1 since the
    /// daemon only requires monotonicity *within a connection window*.
    timestamp_counter: AtomicU32,
}

impl DltBackend {
    /// Start building a backend.
    ///
    /// All required fields (`app`, `default_context`, `ecu_id`, and one
    /// of `uds` / `tcp`) must be set before [`DltBackendBuilder::build`]
    /// is called.
    pub fn builder() -> DltBackendBuilder {
        DltBackendBuilder::default()
    }

    /// Borrow the encoder used by this backend.
    ///
    /// Returned as an [`Arc`] so callers (notably the planned T19
    /// drop-summary record builder) can keep a cheap clone alongside
    /// the backend without duplicating its configuration.
    pub fn encoder(&self) -> Arc<Encoder> {
        Arc::clone(&self.encoder)
    }

    /// Borrow the per-context level table.
    ///
    /// Exposed so callers can pre-seed levels for known contexts before
    /// the first control message arrives. Routine read access happens
    /// inside [`LogSink::enabled`] and does not require touching this.
    pub fn level_table(&self) -> Arc<LevelTable> {
        Arc::clone(&self.level_table)
    }

    /// Borrow the offline ring for **test and diagnostic use only**.
    ///
    /// Production code never pushes into the ring directly — [`LogSink::emit`]
    /// routes overflow there via the `try_send` fall-through path in
    /// accordance with REQ_0814. The accessor is exposed so integration
    /// tests can populate the ring with pre-encoded records and exercise
    /// the reconnect-drain / drop-summary code paths (REQ_0815) without
    /// requiring a real daemon outage.
    pub fn ring(&self) -> Arc<OfflineRing> {
        Arc::clone(&self.ring)
    }

    /// Allocate the next standard-header timestamp tick.
    fn next_timestamp(&self) -> u32 {
        self.timestamp_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Stop the background flusher and wait for it to exit.
    ///
    /// Safe to call multiple times — subsequent calls are no-ops. After
    /// shutdown, [`LogSink::emit`] still encodes records but the bytes
    /// land in the offline ring and are never sent (the flusher that
    /// would have drained them is gone). Callers that intend to keep
    /// logging must rebuild the backend.
    pub fn shutdown(&self) {
        if let Some(h) = self.flusher.lock().unwrap().take() {
            h.shutdown();
        }
    }
}

impl LogSink for DltBackend {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        let target = metadata.target();
        let ctx = derive_ctx_from_target(target);
        let allowed = self.level_table.current(&ctx);
        metadata.level() <= allowed
    }

    fn emit(&self, record: &log::Record<'_>) {
        if !LogSink::enabled(self, record.metadata()) {
            return;
        }
        let bytes = self.encoder.encode(record, self.next_timestamp());

        // Producer side honours REQ_0812 (non-blocking) by using
        // `try_send` rather than `send`. Two failure modes are possible:
        //
        //  * `Full(bytes)` — the bounded channel to the flusher is at
        //    capacity. Route the bytes to the offline ring per
        //    REQ_0814's overflow policy; the ring's drop-oldest
        //    behaviour increments the drop counter that T19 will
        //    later surface as a synthesised summary record.
        //
        //  * `Disconnected(bytes)` — the flusher thread has exited
        //    (after [`Self::shutdown`] or a panic). The ring will never
        //    drain in that case, but pushing the bytes there is still
        //    preferable to panicking: it keeps `emit` infallible and
        //    leaves a record in memory that a debugger or core dump
        //    can inspect.
        match self.tx.try_send(bytes) {
            Ok(()) => {}
            Err(TrySendError::Full(bytes)) | Err(TrySendError::Disconnected(bytes)) => {
                self.ring.push(bytes);
            }
        }
    }

    fn flush(&self) {
        // v1 flush is best-effort: yield the calling thread briefly so
        // the flusher has a chance to drain the channel before the
        // caller (typically a process-exit handler) returns. A precise
        // "all in-flight bytes acknowledged" semantic requires a sync
        // primitive between the producer and the flusher thread, which
        // is deferred to a later task. Documented here so the gap is
        // not silently shipped.
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl log::Log for DltBackend {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        LogSink::enabled(self, metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        LogSink::emit(self, record);
    }

    fn flush(&self) {
        LogSink::flush(self);
    }
}

/// Derive a 4-character DLT context id from a `log` target string.
///
/// Strategy (v1, approximate):
///
///  1. Filter the target to ASCII bytes (DLT ids must be ASCII).
///  2. If the filtered string has at least 4 characters, take the last
///     four — module paths like `tk.test::sub` are dominated by their
///     final segment, which is the discriminating part.
///  3. Otherwise right-align with leading spaces to reach 4 bytes.
///  4. If the resulting id still fails [`CtxId::new`] (e.g. someone
///     constructs a target where the filtered length is wrong), fall
///     back to `"MAIN"` — keeping `emit` infallible.
///
/// This will be revisited once explicit per-target ctx overrides land.
fn derive_ctx_from_target(target: &str) -> CtxId {
    let trimmed: String = target.chars().filter(|c| c.is_ascii()).collect();
    let four = if trimmed.len() >= 4 {
        trimmed[trimmed.len() - 4..].to_string()
    } else {
        format!("{:>4}", trimmed) // right-aligned with leading spaces
    };
    CtxId::new(&four).unwrap_or_else(|_| {
        // `CtxId::new("MAIN")` is statically valid — the unwrap here
        // is reachable only if the const "MAIN" itself ever fails the
        // 4-ASCII-bytes check, which it does not.
        CtxId::new("MAIN").expect("\"MAIN\" is a valid CtxId")
    })
}

/// Builder for [`DltBackend`].
///
/// Created via [`DltBackend::builder`]. All setters are infallible; any
/// required-field validation happens in [`Self::build`].
#[derive(Default)]
pub struct DltBackendBuilder {
    app: Option<AppId>,
    default_ctx: Option<CtxId>,
    ecu_id: Option<String>,
    transport: Option<TransportConfig>,
    ring_capacity: usize,
}

impl DltBackendBuilder {
    /// Set the DLT application id (`APID`) used for every emitted record.
    pub fn app(mut self, id: AppId) -> Self {
        self.app = Some(id);
        self
    }

    /// Set the default DLT context id (`CTID`) used by the encoder for
    /// records whose `log` target does not (yet) map onto a per-target
    /// context override.
    pub fn default_context(mut self, id: CtxId) -> Self {
        self.default_ctx = Some(id);
        self
    }

    /// Set the ECU identifier embedded in every record's storage and
    /// standard headers (e.g. `"ECU1"`).
    pub fn ecu_id(mut self, ecu: impl Into<String>) -> Self {
        self.ecu_id = Some(ecu.into());
        self
    }

    /// Configure the backend to talk to `dlt-daemon` over a
    /// Unix-domain socket at `path`. Mutually exclusive with [`Self::tcp`]
    /// — calling either replaces any previous transport setting.
    pub fn uds(mut self, path: &Path) -> Self {
        self.transport = Some(TransportConfig::Uds(PathBuf::from(path)));
        self
    }

    /// Configure the backend to talk to `dlt-daemon` over TCP at `addr`
    /// (e.g. `"127.0.0.1:3490"`). Mutually exclusive with [`Self::uds`].
    pub fn tcp(mut self, addr: impl Into<String>) -> Self {
        self.transport = Some(TransportConfig::Tcp(addr.into()));
        self
    }

    /// Set the offline-ring capacity (in records).
    ///
    /// `0` is treated as "use the default" (512).
    pub fn ring_capacity(mut self, n: usize) -> Self {
        self.ring_capacity = n;
        self
    }

    /// Finalise the builder, spawning the background flusher and
    /// returning a ready-to-use [`DltBackend`].
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::Missing`] naming the first missing required
    /// field. Required fields are `app`, `default_context`, `ecu_id`,
    /// and one of `uds` / `tcp`.
    pub fn build(self) -> Result<DltBackend, BuildError> {
        let app = self.app.ok_or(BuildError::Missing("app"))?;
        let default_ctx = self
            .default_ctx
            .ok_or(BuildError::Missing("default_context"))?;
        let ecu_id = self.ecu_id.ok_or(BuildError::Missing("ecu_id"))?;
        let transport = self.transport.ok_or(BuildError::Missing("transport"))?;
        let capacity = if self.ring_capacity == 0 {
            DEFAULT_RING_CAPACITY
        } else {
            self.ring_capacity
        };

        let encoder = Arc::new(Encoder::new(app, default_ctx, ecu_id));
        let level_table = Arc::new(LevelTable::new(log::Level::Info));
        let ring = Arc::new(OfflineRing::with_capacity(capacity));

        // REQ_0815: synthesise a `taktora.log.dropped count=N` warn-level
        // record on every reconnect that follows an overflow. We hand
        // the flusher a clone of the encoder so the bytes look
        // indistinguishable from any other DLT record on the wire.
        let encoder_for_summary = Arc::clone(&encoder);
        let summary_builder: SummaryBuilder = Arc::new(move |count: u64| {
            // `body` owns the formatted string; `args` borrows from it.
            // Both live until `encoder.encode` returns the encoded bytes
            // at the end of the closure, so no lifetime extension trickery
            // is required.
            let body = format!("taktora.log.dropped count={count}");
            let args = format_args!("{body}");
            let rec = log::Record::builder()
                .level(log::Level::Warn)
                .target("taktora.log.diag")
                .args(args)
                .build();
            // Timestamp tick = 0 for synthesised summary records. The
            // dlt-daemon's monotonicity expectation is per connection
            // window; the summary leads every post-reconnect drain, so a
            // fresh-from-zero counter does not violate that.
            encoder_for_summary.encode(&rec, 0)
        });

        let (handle, tx) = spawn_flusher(FlusherConfig {
            transport,
            ring: Arc::clone(&ring),
            level_table: Arc::clone(&level_table),
            reconnect_initial_backoff: DEFAULT_RECONNECT_INITIAL_BACKOFF,
            reconnect_max_backoff: DEFAULT_RECONNECT_MAX_BACKOFF,
            summary_builder: Some(summary_builder),
        });

        Ok(DltBackend {
            encoder,
            tx,
            level_table,
            ring,
            flusher: std::sync::Mutex::new(Some(handle)),
            timestamp_counter: AtomicU32::new(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: extract a [`BuildError`] from `Result<DltBackend, _>`
    /// without requiring [`DltBackend`] to implement [`Debug`] (it
    /// holds non-`Debug` internals like the channel sender, encoder,
    /// and flusher handle).
    fn expect_err(r: Result<DltBackend, BuildError>) -> BuildError {
        match r {
            Ok(_) => panic!("expected BuildError, got Ok(DltBackend)"),
            Err(e) => e,
        }
    }

    #[test]
    fn builder_reports_missing_app() {
        let err = expect_err(DltBackend::builder().build());
        assert!(matches!(err, BuildError::Missing("app")));
    }

    #[test]
    fn builder_reports_missing_default_context() {
        let err = expect_err(
            DltBackend::builder()
                .app(AppId::new("TKEX").unwrap())
                .build(),
        );
        assert!(matches!(err, BuildError::Missing("default_context")));
    }

    #[test]
    fn builder_reports_missing_ecu_id() {
        let err = expect_err(
            DltBackend::builder()
                .app(AppId::new("TKEX").unwrap())
                .default_context(CtxId::new("MAIN").unwrap())
                .build(),
        );
        assert!(matches!(err, BuildError::Missing("ecu_id")));
    }

    #[test]
    fn builder_reports_missing_transport() {
        let err = expect_err(
            DltBackend::builder()
                .app(AppId::new("TKEX").unwrap())
                .default_context(CtxId::new("MAIN").unwrap())
                .ecu_id("ECU1")
                .build(),
        );
        assert!(matches!(err, BuildError::Missing("transport")));
    }

    #[test]
    fn derive_ctx_takes_last_four_ascii_chars() {
        assert_eq!(derive_ctx_from_target("tk.test").as_str(), "test");
    }

    #[test]
    fn derive_ctx_right_pads_short_target() {
        assert_eq!(derive_ctx_from_target("tk").as_str(), "  tk");
    }

    #[test]
    fn derive_ctx_filters_non_ascii_and_falls_back() {
        // Non-ASCII chars are stripped first, then the residual is
        // right-padded with leading spaces to reach exactly four bytes.
        // Here the residual is the single ASCII byte 'x', so the
        // padded id is "   x".
        assert_eq!(derive_ctx_from_target("\u{00e9}x").as_str(), "   x");
    }
}
