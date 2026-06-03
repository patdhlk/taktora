//! Fatal-handler API — `FatalContext`, `FatalSite`, `FatalHandler`, and the
//! crate-internal `FatalDispatch` that owns the handler plus a swappable
//! terminal action (production: `std::process::abort`; test: recording stub).
//!
//! This module also houses `panic_payload_message`, moved here from
//! `executor.rs` (Task 1) because it is the natural extraction point for
//! panic-payload introspection shared by the fatal path.

// `FatalDispatch` fields and `fire` are wired in Task 3.
#![allow(dead_code)]
// This is a private module; pub(crate) on items is intentional — they are used
// by executor.rs / pool.rs once Task 3 wires the hot path.
#![allow(clippy::redundant_pub_crate)]

use std::sync::Arc;

// ── Public API ────────────────────────────────────────────────────────────────

/// Why the runtime is about to abort. Passed to the fatal handler.
///
/// Marked `#[non_exhaustive]` so future fields (e.g. a thread name, a
/// stack-trace fragment) do not break existing match/struct-init expressions.
#[non_exhaustive]
pub struct FatalContext {
    /// Best-effort message extracted from the panic payload.
    pub cause: String,
    /// Which runtime boundary caught it.
    pub site: FatalSite,
}

/// Which executor boundary detected the unrecoverable fault.
///
/// Marked `#[non_exhaustive]` so new boundaries (e.g. a future timer thread)
/// can be added without breaking `match` arms in caller code.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalSite {
    /// A pool worker thread's `catch_unwind` boundary re-panicked.
    PoolWorker,
    /// The inline-submit path (pool size 0) caught a second panic.
    InlineSubmit,
    /// The executor's main run-loop caught an unrecoverable panic.
    ExecutorRunLoop,
}

/// An `Arc`-wrapped callback invoked once on the fail-fast path.
///
/// **Contract** (from [`ExecutorBuilder::on_fatal`]):
/// - Runs over known-unsound executor state.
/// - MUST NOT touch executor internals.
/// - A panic inside the handler routes straight to `abort()`.
pub type FatalHandler = Arc<dyn Fn(&FatalContext) + Send + Sync + 'static>;

// ── crate-internal dispatch ────────────────────────────────────────────────────

/// Owns a user handler and a terminal action. The terminal is
/// `std::process::abort` in production; tests may substitute a recording stub
/// via `FatalDispatch::with_terminal` (available in `#[cfg(test)]` only).
pub(crate) struct FatalDispatch {
    handler: FatalHandler,
    terminal: Arc<dyn Fn(&FatalContext) + Send + Sync + 'static>,
}

impl FatalDispatch {
    /// Production constructor. Terminal is `std::process::abort`.
    pub(crate) fn new(handler: FatalHandler) -> Self {
        Self {
            handler,
            terminal: Arc::new(|_ctx| std::process::abort()),
        }
    }

    /// Test-only constructor that allows substituting the terminal.
    ///
    /// Only available in `cfg(test)` builds — the abort terminal is the only
    /// terminal reachable in release.
    #[cfg(test)]
    pub(crate) fn with_terminal(
        handler: FatalHandler,
        terminal: impl Fn(&FatalContext) + Send + Sync + 'static,
    ) -> Self {
        Self {
            handler,
            terminal: Arc::new(terminal),
        }
    }

    /// Return a reference to the stored handler.
    ///
    /// Only available in test builds — the handler is an implementation detail;
    /// production code has no need to inspect it.
    #[cfg(test)]
    pub(crate) fn handler(&self) -> &FatalHandler {
        &self.handler
    }

    /// Invoke the handler (catch-guarded so a handler panic still reaches the
    /// terminal), then invoke the terminal.
    ///
    /// In production the terminal calls `std::process::abort()` and therefore
    /// this function diverges. In tests the terminal records and returns.
    pub(crate) fn fire(&self, ctx: &FatalContext) {
        // SAFETY: on the production path the terminal calls std::process::abort()
        // and the process never resumes use of `ctx` or any state captured by the
        // handler closure, so inconsistent state is never observable. The test
        // terminal returns, but test closures hold no cross-unwind invariants.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.handler)(ctx)));
        // Production terminal diverges (abort). Test terminal records + returns.
        (self.terminal)(ctx);
    }
}

// ── Shared helper (moved from executor.rs Task 1) ─────────────────────────────

/// Extract a human-readable message from a panic payload.
///
/// Returns `Some(msg)` when the payload is a `&str` or `String`, and `None`
/// for any other payload type.  Callers may supply their own fallback for the
/// `None` case, which makes the helper reusable across different catch-unwind
/// boundaries that may want different default messages.
pub(crate) fn panic_payload_message(payload: &(dyn core::any::Any + Send)) -> Option<String> {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // ── panic_payload_message ─────────────────────────────────────────────────

    #[test]
    fn panic_payload_message_str_payload() {
        let payload = std::panic::catch_unwind(|| panic!("static str msg")).unwrap_err();
        assert_eq!(
            panic_payload_message(&*payload),
            Some("static str msg".to_string())
        );
    }

    #[test]
    fn panic_payload_message_string_payload() {
        let msg = "owned string msg".to_string();
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| panic!("{}", msg)))
            .unwrap_err();
        assert_eq!(
            panic_payload_message(&*payload),
            Some("owned string msg".to_string())
        );
    }

    #[test]
    fn panic_payload_message_non_string_payload() {
        let payload = std::panic::catch_unwind(|| std::panic::panic_any(42_u32)).unwrap_err();
        assert_eq!(panic_payload_message(&*payload), None);
    }

    // ── FatalDispatch ─────────────────────────────────────────────────────────

    /// Helper: build a recording terminal + a shared log Vec.
    fn recording_terminal() -> (
        Arc<Mutex<Vec<String>>>,
        impl Fn(&FatalContext) + Send + Sync + 'static,
    ) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let terminal = move |ctx: &FatalContext| {
            log2.lock().unwrap().push(format!("terminal:{}", ctx.cause));
        };
        (log, terminal)
    }

    #[test]
    fn fire_runs_handler_then_terminal_in_order() {
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let order_h = Arc::clone(&order);
        let handler: FatalHandler = Arc::new(move |_ctx| {
            order_h.lock().unwrap().push("handler");
        });

        let order_t = Arc::clone(&order);
        let terminal = move |_ctx: &FatalContext| {
            order_t.lock().unwrap().push("terminal");
        };

        let dispatch = FatalDispatch::with_terminal(handler, terminal);
        dispatch.fire(&FatalContext {
            cause: "boom".to_string(),
            site: FatalSite::PoolWorker,
        });

        let log = order.lock().unwrap();
        assert_eq!(
            *log,
            vec!["handler", "terminal"],
            "handler must run before terminal"
        );
    }

    #[test]
    fn fire_handler_panic_still_reaches_terminal() {
        let (log, terminal) = recording_terminal();

        let panicking_handler: FatalHandler = Arc::new(|_ctx| panic!("handler exploded"));

        let dispatch = FatalDispatch::with_terminal(panicking_handler, terminal);
        dispatch.fire(&FatalContext {
            cause: "cause-xyz".to_string(),
            site: FatalSite::ExecutorRunLoop,
        });

        let entries = log.lock().unwrap();
        // Terminal must have been reached even though handler panicked.
        assert!(
            entries.iter().any(|e| e.contains("terminal:cause-xyz")),
            "terminal not reached after handler panic; log: {:?}",
            *entries
        );
    }

    #[test]
    fn fire_default_noop_handler_reaches_terminal() {
        let (log, terminal) = recording_terminal();

        // No-op handler — same as what ExecutorBuilder::build produces when
        // on_fatal is not called.
        let noop: FatalHandler = Arc::new(|_ctx| {});

        let dispatch = FatalDispatch::with_terminal(noop, terminal);
        dispatch.fire(&FatalContext {
            cause: "default".to_string(),
            site: FatalSite::InlineSubmit,
        });

        let entries = log.lock().unwrap();
        assert!(
            entries.iter().any(|e| e.contains("terminal:default")),
            "terminal not reached for default no-op handler; log: {:?}",
            *entries
        );
    }
}
