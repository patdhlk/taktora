//! [`MqttGateway`] — owns the per-connector tokio runtime that hosts the
//! dispatcher loop and (in M3) the `rumqttc` event loop. `REQ_0258`.
//!
//! The runtime is contained entirely inside this crate: no `tokio::` type
//! appears in the public API surface, so tokio never leaks into
//! taktora-executor's WaitSet thread. Mirrors
//! `taktora_connector_zenoh::gateway::ZenohGateway` — same
//! shutdown-budget-on-`Drop` semantics and the same private `Handle`
//! accessor / public `spawn` split.

use std::time::Duration;

use tokio::runtime::{Builder as RtBuilder, Handle, Runtime};

/// Default shutdown budget when joining the gateway's tokio runtime on
/// `Drop`. Mirrors `ZenohGateway::DEFAULT_SHUTDOWN_BUDGET`.
pub const DEFAULT_SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);

/// Gateway-side container. Owns the tokio runtime (`REQ_0258`); the
/// dispatcher loop spawned on this runtime drives all MQTT I/O.
#[derive(Debug)]
pub struct MqttGateway {
    runtime: Option<Runtime>,
    shutdown_budget: Duration,
}

impl MqttGateway {
    /// Construct the gateway and start its tokio runtime with
    /// `worker_threads` worker threads (clamped to at least 1).
    ///
    /// # Errors
    ///
    /// Returns the tokio runtime error verbatim if runtime construction
    /// fails (e.g. the OS denies thread creation under resource pressure).
    pub fn new(worker_threads: usize) -> std::io::Result<Self> {
        let runtime = build_runtime(worker_threads)?;
        Ok(Self {
            runtime: Some(runtime),
            shutdown_budget: DEFAULT_SHUTDOWN_BUDGET,
        })
    }

    /// Construct the gateway with a custom shutdown budget. Useful in tests
    /// that want a tighter teardown timeout.
    ///
    /// # Errors
    ///
    /// Same as [`Self::new`].
    pub fn with_shutdown_budget(worker_threads: usize, budget: Duration) -> std::io::Result<Self> {
        let mut gw = Self::new(worker_threads)?;
        gw.shutdown_budget = budget;
        Ok(gw)
    }

    /// Borrow a tokio [`Handle`] for spawning work on the gateway's runtime.
    /// Returns `None` after `Drop` has consumed the runtime.
    ///
    /// Crate-internal only — the public [`Self::spawn`] method is the
    /// supported entry point. Keeping the handle off the public surface
    /// preserves `REQ_0258` (no `tokio::` types in the public API of
    /// `taktora-connector-mqtt`).
    #[must_use]
    pub(crate) fn handle(&self) -> Option<Handle> {
        self.runtime.as_ref().map(Runtime::handle).cloned()
    }

    /// Spawn `fut` on the gateway's tokio runtime without exposing any
    /// `tokio::` type in the signature (`REQ_0258`).
    ///
    /// The returned `JoinHandle` is dropped internally — call sites observe
    /// task progress through their own channels.
    ///
    /// # Panics
    ///
    /// Panics if the runtime has already been consumed by `Drop`. This does
    /// not happen in normal use because `spawn` takes `&self` while `Drop`
    /// takes `&mut self`.
    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = self
            .runtime
            .as_ref()
            .expect("runtime not yet dropped")
            .handle();
        drop(handle.spawn(fut));
    }

    /// Shutdown budget honoured by the `Drop` impl.
    #[must_use]
    pub const fn shutdown_budget(&self) -> Duration {
        self.shutdown_budget
    }
}

impl Drop for MqttGateway {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(self.shutdown_budget);
        }
    }
}

fn build_runtime(worker_threads: usize) -> std::io::Result<Runtime> {
    let mut builder = RtBuilder::new_multi_thread();
    builder.worker_threads(worker_threads.max(1));
    builder.thread_name("taktora-mqtt");
    builder.enable_all();
    builder.build()
}
