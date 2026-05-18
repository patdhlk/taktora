//! Backend extension surface — implemented by every `taktora-log` backend.
//!
//! See REQ_0802 in `spec/requirements/logging.rst`.

use log::Record;

/// A logging backend.
///
/// Implementations encode a `log::Record` to whatever wire format the
/// backend speaks and ship it. The trait is object-safe so that the
/// facade can hold a `Box<dyn LogSink>` selected at runtime by the
/// [`crate::Builder`].
///
/// # Backend responsibilities
///
/// * `emit` — encode and dispatch one record. Must not block the
///   calling thread (REQ_0812); push to an internal queue and let a
///   background flusher handle I/O.
/// * `enabled` — fast level check used by `log::Log::enabled`.
///   Implementations should short-circuit against an atomic per-target
///   level table where applicable (REQ_0810).
/// * `flush` — drain any pending records. Called on process shutdown.
///
/// Implementations are typically `Send + Sync` so the facade can wrap
/// them in `Arc` and install them as the global `log::Log`.
pub trait LogSink: Send + Sync {
    /// Returns `true` if `metadata` would be emitted at the current
    /// runtime level for its target.
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool;

    /// Emit one record. Must not block.
    fn emit(&self, record: &Record<'_>);

    /// Drain any pending records. Called on process shutdown and may
    /// block for a bounded time.
    fn flush(&self);
}
