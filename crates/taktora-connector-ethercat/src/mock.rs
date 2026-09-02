//! [`MockBusDriver`] — programmable [`BusDriver`] implementation.
//!
//! Always compiled (cheap; no external deps) so downstream crates can
//! also use it for their own connector tests.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use taktora_connector_core::ConnectorError;

use crate::driver::{BringUp, BusDriver};

/// Which cycle method a `cycle` call represents.
///
/// Set on the mock via [`MockBusDriver::with_dc_cycle_kind`]; recorded
/// into `MockState::cycle_kinds` on every `cycle` call. Used by tests
/// asserting the DC branch (`REQ_0330`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CycleKind {
    /// `group.tx_rx` — the non-DC code path.
    #[default]
    Plain,
    /// `group.tx_rx_dc` — the DC opt-in code path.
    Dc,
}

/// Programmable [`BusDriver`] for tests. Records every method call
/// and lets tests preload sequences of return values.
///
/// PDI buffers (outputs / inputs) live behind `Mutex` so the trait's
/// callback-shaped `with_subdevice_outputs_mut` /
/// `with_subdevice_inputs` can invoke their closures with the buffer
/// locked. Configure the buffers via
/// [`MockBusDriver::with_subdevice_outputs`] /
/// [`MockBusDriver::with_subdevice_inputs`].
#[derive(Debug, Default)]
pub struct MockBusDriver {
    state: Mutex<MockState>,
    subdevice_outputs: Mutex<HashMap<u16, Vec<u8>>>,
    subdevice_inputs: Mutex<HashMap<u16, Vec<u8>>>,
}

#[derive(Debug, Default)]
struct MockState {
    /// `Some(reason)` makes the next `bring_up` fail with
    /// [`ConnectorError::Down`] carrying `reason`.
    bring_up_fails: Option<String>,
    /// Returned by `bring_up`. Defaults to `expected_wkc = 3`,
    /// `subdevice_count = 1` so simple tests don't have to configure.
    bring_up_response: BringUp,
    /// Number of `bring_up` calls that have completed (success or
    /// failure). Useful in test assertions.
    bring_up_calls: u32,
    /// Per-`cycle` working counters, drained FIFO. When empty, every
    /// subsequent `cycle` call returns [`Self::default_cycle_wkc`].
    wkc_sequence: VecDeque<u16>,
    /// Fallback WKC when `Self::with_wkc_sequence` is empty.
    default_cycle_wkc: u16,
    /// Number of `cycle` calls that have completed.
    cycle_calls: u32,
    /// When `true`, every `cycle` call copies each SubDevice's
    /// outputs buffer over its inputs buffer (synthetic loopback).
    /// Used by `TEST_0222`.
    loopback: bool,
    /// Programmed sequence of `recover` outcomes, drained FIFO. When
    /// empty, every subsequent `recover` returns
    /// `Ok(bring_up_response)` (mirroring the existing
    /// `default_cycle_wkc` fallback).
    recovery_sequence: VecDeque<Result<BringUp, String>>,
    /// Number of `recover` calls completed.
    recover_calls: u32,
    /// Default `CycleKind` recorded on every `cycle` call. Defaults to
    /// `Plain`; set to `Dc` via [`MockBusDriver::with_dc_cycle_kind`].
    cycle_kind: CycleKind,
    /// Every cycle call's recorded kind, ordered earliest-first.
    cycle_kinds: Vec<CycleKind>,
    /// When set, the `cycle_calls`-th cycle call (1-indexed,
    /// matching the post-increment value) returns
    /// `Err(ConnectorError::Down { reason })`. Cleared once it
    /// fires.
    cycle_err_after: Option<(u32, String)>,
}

impl MockBusDriver {
    /// Construct a driver with sensible defaults — `bring_up` succeeds
    /// with `expected_wkc = 3`; every `cycle` returns `3`. No PDI
    /// buffers are configured by default; configure them via
    /// [`Self::with_subdevice_outputs`] /
    /// [`Self::with_subdevice_inputs`] when testing C7a / C7b
    /// integration paths.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MockState {
                bring_up_response: BringUp {
                    expected_wkc: 3,
                    subdevice_count: 1,
                },
                default_cycle_wkc: 3,
                ..Default::default()
            }),
            subdevice_outputs: Mutex::new(HashMap::new()),
            subdevice_inputs: Mutex::new(HashMap::new()),
        }
    }

    /// Configure the outputs buffer for a synthetic SubDevice at
    /// `address`. The buffer is initialised to the supplied bytes;
    /// the dispatcher (C7b) writes through
    /// [`BusDriver::with_subdevice_outputs_mut`].
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the internal mutex by
    /// panicking while holding it (build-only helper; not reached
    /// in well-behaved tests).
    #[must_use]
    pub fn with_subdevice_outputs(self, address: u16, initial: Vec<u8>) -> Self {
        self.subdevice_outputs
            .lock()
            .expect("not poisoned")
            .insert(address, initial);
        self
    }

    /// Configure the inputs buffer for a synthetic SubDevice at
    /// `address`. Used by inbound-path tests to drive synthetic PDI
    /// inputs that the dispatcher will read via
    /// [`BusDriver::with_subdevice_inputs`].
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the internal mutex by
    /// panicking while holding it.
    #[must_use]
    pub fn with_subdevice_inputs(self, address: u16, initial: Vec<u8>) -> Self {
        self.subdevice_inputs
            .lock()
            .expect("not poisoned")
            .insert(address, initial);
        self
    }

    /// Configure the [`BringUp`] response.
    #[must_use]
    pub fn with_bring_up(self, response: BringUp) -> Self {
        self.lock().bring_up_response = response;
        self
    }

    /// Make the next `bring_up` call fail with [`ConnectorError::Down`]
    /// carrying `reason`.
    #[must_use]
    pub fn failing_bring_up(self, reason: impl Into<String>) -> Self {
        self.lock().bring_up_fails = Some(reason.into());
        self
    }

    /// Override the fallback `cycle` WKC (used after
    /// `Self::with_wkc_sequence` is drained).
    #[must_use]
    pub fn with_default_cycle_wkc(self, wkc: u16) -> Self {
        self.lock().default_cycle_wkc = wkc;
        self
    }

    /// Preload a sequence of WKC values to return from successive
    /// `cycle` calls (FIFO).
    #[must_use]
    pub fn with_wkc_sequence<I>(self, seq: I) -> Self
    where
        I: IntoIterator<Item = u16>,
    {
        self.lock().wkc_sequence = seq.into_iter().collect();
        self
    }

    /// Preload a sequence of `recover` outcomes (FIFO). Each `Err`
    /// element is surfaced as `ConnectorError::Down { reason }`; each
    /// `Ok(BringUp)` is returned verbatim. When the sequence is
    /// exhausted, subsequent calls fall back to
    /// `Ok(bring_up_response)`.
    #[must_use]
    pub fn with_recovery_sequence<I, S>(self, seq: I) -> Self
    where
        I: IntoIterator<Item = Result<BringUp, S>>,
        S: Into<String>,
    {
        self.lock().recovery_sequence = seq.into_iter().map(|r| r.map_err(Into::into)).collect();
        self
    }

    /// Make the `n`-th cycle call (1-indexed) fail with `reason`. The
    /// failure fires exactly once; subsequent cycles fall through to
    /// the wkc_sequence / default_cycle_wkc fallback. Used by tests
    /// that need to drive the cycle runner's recovery loop
    /// (`REQ_0331` / `REQ_0333`).
    #[must_use]
    pub fn with_cycle_err_after(self, n: u32, reason: impl Into<String>) -> Self {
        self.lock().cycle_err_after = Some((n, reason.into()));
        self
    }

    /// Number of `recover` calls completed since construction.
    pub fn recover_calls(&self) -> u32 {
        self.lock().recover_calls
    }

    /// Enable synthetic loopback: every subsequent `cycle` call
    /// copies each SubDevice's outputs buffer over its inputs
    /// buffer. Used by `TEST_0222` to exercise the full
    /// iceoryx2 ↔ PDI ↔ iceoryx2 round-trip without hardware. The
    /// outputs and inputs buffers for the same SubDevice address
    /// must both be configured (via
    /// [`Self::with_subdevice_outputs`] / [`Self::with_subdevice_inputs`]);
    /// the inputs buffer is resized to match outputs on each cycle
    /// if the lengths differ.
    #[must_use]
    pub fn with_loopback(self) -> Self {
        self.lock().loopback = true;
        self
    }

    /// Mark every subsequent `cycle` call as DC. Used by tests that
    /// stand in for the production driver's DC branch behaviour.
    #[must_use]
    pub fn with_dc_cycle_kind(self) -> Self {
        self.lock().cycle_kind = CycleKind::Dc;
        self
    }

    /// Snapshot of every cycle kind recorded since construction
    /// (earliest first).
    #[must_use]
    pub fn cycle_kinds(&self) -> Vec<CycleKind> {
        self.lock().cycle_kinds.clone()
    }

    /// Snapshot the outputs buffer for `address`. Returns `None`
    /// when no buffer was configured for that SubDevice.
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the internal mutex by
    /// panicking while holding it (build-only helper; not reached
    /// in well-behaved tests).
    #[must_use]
    pub fn snapshot_outputs(&self, address: u16) -> Option<Vec<u8>> {
        self.subdevice_outputs
            .lock()
            .expect("not poisoned")
            .get(&address)
            .cloned()
    }

    /// Snapshot the inputs buffer for `address`. Returns `None`
    /// when no buffer was configured for that SubDevice.
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the internal mutex by
    /// panicking while holding it.
    #[must_use]
    pub fn snapshot_inputs(&self, address: u16) -> Option<Vec<u8>> {
        self.subdevice_inputs
            .lock()
            .expect("not poisoned")
            .get(&address)
            .cloned()
    }

    /// Number of `bring_up` calls completed since construction.
    pub fn bring_up_calls(&self) -> u32 {
        self.lock().bring_up_calls
    }

    /// Number of `cycle` calls completed since construction.
    pub fn cycle_calls(&self) -> u32 {
        self.lock().cycle_calls
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state.lock().expect("MockBusDriver mutex not poisoned")
    }
}

impl BusDriver for MockBusDriver {
    // `BusDriver` returns `impl Future` because the ethercrab driver awaits the
    // wire; the mock answers from memory, so each method does its work eagerly
    // and hands back an already-resolved future.
    fn bring_up(
        &mut self,
    ) -> impl core::future::Future<Output = Result<BringUp, ConnectorError>> + Send + '_ {
        core::future::ready((|| -> Result<BringUp, ConnectorError> {
            let mut state = self.lock();
            state.bring_up_calls += 1;
            if let Some(reason) = state.bring_up_fails.take() {
                return Err(ConnectorError::Down { reason });
            }
            Ok(state.bring_up_response)
        })())
    }

    fn cycle(
        &mut self,
    ) -> impl core::future::Future<Output = Result<u16, ConnectorError>> + Send + '_ {
        core::future::ready((|| -> Result<u16, ConnectorError> {
            // Pre-increment to compute the target index. Check
            // cycle_err_after BEFORE recording cycle_kind / pushing kinds /
            // draining wkc_sequence so a programmed error counts as one
            // cycle but produces no other visible side-effects on this
            // call. Splitting the critical sections (rather than folding
            // the err check into the original one) keeps the WKC + kind
            // recording cleanly skipped on the failure path.
            let err = {
                let mut state = self.lock();
                state.cycle_calls += 1;
                if let Some((target, _)) = state.cycle_err_after.as_ref() {
                    if state.cycle_calls == *target {
                        state.cycle_err_after.take().map(|(_, reason)| reason)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(reason) = err {
                return Err(ConnectorError::Down { reason });
            }
            let (wkc, loopback) = {
                let mut state = self.lock();
                let kind = state.cycle_kind;
                state.cycle_kinds.push(kind);
                let wkc = state
                    .wkc_sequence
                    .pop_front()
                    .unwrap_or(state.default_cycle_wkc);
                (wkc, state.loopback)
            };
            if loopback {
                // Two short critical sections: snapshot outputs, then
                // write them into inputs. Splitting the locks keeps each
                // critical section bounded.
                let outputs_snapshot: Vec<(u16, Vec<u8>)> = {
                    let guard = self.subdevice_outputs.lock().expect("not poisoned");
                    guard.iter().map(|(a, b)| (*a, b.clone())).collect()
                };
                let mut inputs = self.subdevice_inputs.lock().expect("not poisoned");
                for (addr, bytes) in outputs_snapshot {
                    let entry = inputs.entry(addr).or_default();
                    if entry.len() != bytes.len() {
                        entry.resize(bytes.len(), 0);
                    }
                    entry.copy_from_slice(&bytes);
                }
                drop(inputs);
            }
            Ok(wkc)
        })())
    }

    fn recover(
        &mut self,
    ) -> impl core::future::Future<Output = Result<BringUp, ConnectorError>> + Send + '_ {
        core::future::ready({
            let mut state = self.lock();
            state.recover_calls += 1;
            match state.recovery_sequence.pop_front() {
                Some(Ok(b)) => Ok(b),
                Some(Err(reason)) => Err(ConnectorError::Down { reason }),
                None => Ok(state.bring_up_response),
            }
        })
    }

    fn with_subdevice_outputs_mut<R>(
        &self,
        subdevice_address: u16,
        f: impl FnOnce(&mut [u8]) -> R,
    ) -> Option<R> {
        let mut guard = self.subdevice_outputs.lock().expect("not poisoned");
        guard
            .get_mut(&subdevice_address)
            .map(|buf| f(buf.as_mut_slice()))
    }

    fn with_subdevice_inputs<R>(
        &self,
        subdevice_address: u16,
        f: impl FnOnce(&[u8]) -> R,
    ) -> Option<R> {
        let guard = self.subdevice_inputs.lock().expect("not poisoned");
        guard.get(&subdevice_address).map(|buf| f(buf.as_slice()))
    }
}
