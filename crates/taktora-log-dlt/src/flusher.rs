//! Background flusher: owns the socket, drains the producer channel
//! and the offline ring, applies control messages.
//!
//! REQ_0812: producer does not block — the flusher does the I/O.
//! REQ_0814: the offline ring is drained FIFO on reconnect.
//! REQ_0815: the drop-summary record is emitted at the head of the
//! drain after an overflow event. The flusher itself does not author
//! the record; the caller supplies a [`SummaryBuilder`] callback that
//! synthesises the encoded bytes from
//! [`OfflineRing::drops_since_last_drain`]. The flusher snapshots the
//! drop count immediately before draining the ring, writes the
//! synthesised summary first, then writes the drained records. If no
//! `summary_builder` is configured, the leading summary is skipped —
//! useful for unit tests that drive the flusher with raw byte
//! payloads and have no notion of a `log::Record`.
//!
//! ## Concurrency model
//!
//! [`spawn_flusher`] starts a single OS thread that owns the socket
//! handle, the read buffer, and the current backoff value. All shared
//! state ([`OfflineRing`], [`LevelTable`]) is already thread-safe.
//! The producer side communicates via a bounded
//! [`crossbeam_channel`] — see [`PRODUCER_QUEUE_CAP`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::level_table::LevelTable;
use crate::ring::OfflineRing;
use crate::transport::{Transport, TransportConfig, TransportError};

/// Maximum number of in-flight encoded records held between the
/// producer (`log::Log::log`) and the flusher thread.
///
/// When the channel is full, the producer falls back to the
/// [`OfflineRing`] so the call site still never blocks (REQ_0812).
pub const PRODUCER_QUEUE_CAP: usize = 4096;

/// Read timeout applied to the daemon socket so the flusher's poll
/// loop can return to its `stop` and `rx` checks. Short enough that
/// shutdown latency stays bounded; long enough that we don't spin.
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(100);

/// Receive timeout used between channel polls. Same justification as
/// [`SOCKET_READ_TIMEOUT`] — bounds shutdown latency.
const CHANNEL_RECV_TIMEOUT: Duration = Duration::from_millis(100);

/// Type alias for the optional summary-record builder callback held on
/// [`FlusherConfig::summary_builder`]. Takes the snapshot drop count
/// and returns the already-encoded DLT bytes to emit.
pub type SummaryBuilder = Arc<dyn Fn(u64) -> Vec<u8> + Send + Sync>;

/// Configuration for [`spawn_flusher`].
pub struct FlusherConfig {
    /// How to reach `dlt-daemon` (UDS path or `host:port`).
    pub transport: TransportConfig,
    /// Offline buffer used while the daemon is unreachable (REQ_0814).
    pub ring: Arc<OfflineRing>,
    /// Per-context level table updated by daemon control messages (REQ_0810).
    pub level_table: Arc<LevelTable>,
    /// Initial wait between reconnect attempts. Doubles on every failed
    /// connect up to [`Self::reconnect_max_backoff`].
    pub reconnect_initial_backoff: Duration,
    /// Upper bound for the exponential reconnect backoff.
    pub reconnect_max_backoff: Duration,
    /// Optional callback that synthesises a drop-summary record
    /// (REQ_0815) from the snapshot of
    /// [`OfflineRing::drops_since_last_drain`].
    ///
    /// Invoked on every successful reconnect where the snapshot is
    /// non-zero. The returned byte buffer is written to the new
    /// connection **before** the buffered ring contents so the summary
    /// always sits at the leading position of the post-reconnect drain.
    ///
    /// If the builder is `None`, no summary is emitted — useful for
    /// callers (e.g. unit tests) that drive the flusher with raw bytes
    /// rather than encoded `log::Record`s and have no concept of a
    /// summary message.
    ///
    /// Contract for the callback:
    ///
    /// * It must produce a fully-encoded DLT record (storage header +
    ///   standard header + payload) — the flusher writes the buffer
    ///   verbatim, exactly as it does for entries drained from the ring.
    /// * It is called from the flusher thread; the closure must be
    ///   `Send + Sync` and should avoid blocking I/O.
    ///
    /// Recovery semantics on summary-write failure: the flusher writes
    /// the summary *before* calling [`OfflineRing::drain_all`], so a
    /// failed write leaves the drop counter intact. The next reconnect
    /// observes the same drop count and re-attempts the summary.
    pub summary_builder: Option<SummaryBuilder>,
}

/// Handle to a running flusher thread.
///
/// Dropping the handle without calling [`Self::shutdown`] leaves the
/// flusher running until the producer side of the channel is dropped.
pub struct FlusherHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl FlusherHandle {
    /// Stop the flusher and wait for the thread to exit.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

/// Spawn the background flusher.
///
/// Returns a handle plus the producer sender — callers push
/// already-encoded DLT bytes into `tx`. The channel is bounded at
/// [`PRODUCER_QUEUE_CAP`]; a full channel is the producer's signal to
/// route into the [`OfflineRing`] instead (handled by Task 17).
pub fn spawn_flusher(cfg: FlusherConfig) -> (FlusherHandle, Sender<Vec<u8>>) {
    let (tx, rx) = bounded::<Vec<u8>>(PRODUCER_QUEUE_CAP);
    let stop = Arc::new(AtomicBool::new(false));
    let join = thread::Builder::new()
        .name("taktora-log-dlt-flusher".into())
        .spawn({
            let stop = Arc::clone(&stop);
            move || run(cfg, rx, stop)
        })
        .expect("spawn flusher thread");
    (
        FlusherHandle {
            stop,
            join: Some(join),
        },
        tx,
    )
}

/// Outcome of [`try_connect`]: either a freshly-connected transport (TCP
/// up and read-timeout configured, but the post-connect summary/drain
/// steps NOT yet run) or an indication that the connect-level attempt
/// failed and the caller should back off and retry.
///
/// The summary (REQ_0815) and drain (REQ_0814) steps are deliberately
/// owned by [`run`] rather than by `try_connect`: the original loop
/// applies *different* backoff timing to a connect-level failure versus a
/// post-connect write failure (a connect failure sleeps then grows the
/// backoff; a summary-write failure grows the backoff but retries
/// immediately with no sleep; a mid-drain write failure simply drops the
/// transport and falls through with the backoff already reset). Folding
/// those steps into a single `Retry` arm would normalise that
/// intentionally-asymmetric timing, so they stay in `run`.
enum ConnectOutcome {
    /// TCP connect succeeded and the read timeout is configured. The
    /// caller still owes the summary + drain steps before treating the
    /// transport as fully online.
    Connected(Transport),
    /// Connect or `set_read_timeout` failed; caller should sleep for the
    /// current backoff, grow it, and retry.
    Retry,
}

/// Attempt the connect-level portion of one reconnect cycle: open the
/// transport and configure its read timeout.
///
/// Returns [`ConnectOutcome::Connected`] when both succeed. A failure of
/// either step returns [`ConnectOutcome::Retry`]; in the original loop
/// both of these paths sleep for the current backoff and then grow it, so
/// the caller treats them identically.
fn try_connect(cfg: &FlusherConfig) -> ConnectOutcome {
    let mut t = match Transport::connect(&cfg.transport) {
        Ok(t) => t,
        Err(_) => return ConnectOutcome::Retry,
    };

    // Best-effort: a missing read timeout just means the control-poll
    // path may block until daemon sends data.  Production daemons send
    // heartbeats; tests don't, so failure to set this would hang the test.
    if t.set_read_timeout(SOCKET_READ_TIMEOUT).is_err() {
        return ConnectOutcome::Retry;
    }

    ConnectOutcome::Connected(t)
}

/// Write the drop-summary record to `t` if one is warranted (REQ_0815).
///
/// Returns `Ok(())` when either no summary is needed or the write
/// succeeded.  Returns `Err(())` on a write failure; the caller should
/// then discard `t` and retry.
fn emit_drop_summary(t: &mut Transport, cfg: &FlusherConfig) -> Result<(), ()> {
    let drops = cfg.ring.drops_since_last_drain();
    let summary_bytes = if drops > 0 {
        cfg.summary_builder.as_ref().map(|b| b(drops))
    } else {
        None
    };

    if let Some(s) = summary_bytes {
        // On failure the drop counter is left intact (drain_all has not
        // run yet) so the next reconnect re-attempts the summary with
        // the same count.
        t.write_all(&s).map_err(|_| ())?;
    }
    Ok(())
}

/// Drain every record from the offline ring into `t` (REQ_0814, FIFO).
///
/// On a write failure the failed record and every remaining un-sent item
/// are re-buffered into the ring so the next reconnect can retry them.
/// Returns `Ok(())` when all records were sent, `Err(())` on failure.
fn drain_ring_into_transport(t: &mut Transport, cfg: &FlusherConfig) -> Result<(), ()> {
    let drained = cfg.ring.drain_all();
    let mut iter = drained.into_iter();
    while let Some(bytes) = iter.next() {
        if t.write_all(&bytes).is_err() {
            // Re-buffer the failed record first, then every remaining
            // undrained item — preserves FIFO across the reconnect
            // attempt.  Note: concurrent producers that pushed to the
            // ring during the drain window sit AHEAD of this re-buffered
            // remainder on the next drain.  That ordering blemish is
            // acceptable for v1 — strict cross-reconnect FIFO is not
            // required by the spec (REQ_0814).
            cfg.ring.push(bytes);
            for rest in iter.by_ref() {
                cfg.ring.push(rest);
            }
            return Err(());
        }
    }
    Ok(())
}

/// Handle one DLT record received from the producer channel.
///
/// Writes `bytes` to `transport` if connected; on write failure the
/// record is routed to the offline ring and `transport` is cleared so
/// the next loop iteration reconnects.  If `transport` is already
/// `None`, the record is routed to the ring directly.
fn handle_channel_bytes(bytes: Vec<u8>, transport: &mut Option<Transport>, cfg: &FlusherConfig) {
    if let Some(t) = transport.as_mut() {
        if t.write_all(&bytes).is_err() {
            // Daemon dropped — buffer this record and reconnect.
            cfg.ring.push(bytes);
            *transport = None;
        }
    } else {
        cfg.ring.push(bytes);
    }
}

/// Poll the transport for an incoming DLT control message (idle path).
///
/// Called when the producer channel times out with no new records.
/// Reads at most one message per call (v1 — one control message per
/// read; a future iteration adds framing).  Clears `transport` on an
/// orderly daemon shutdown (`Ok(0)`) or an unexpected I/O error.
fn handle_idle(transport: &mut Option<Transport>, cfg: &FlusherConfig, read_buf: &mut [u8]) {
    let Some(t) = transport.as_mut() else { return };
    match t.read(read_buf) {
        Ok(0) => {
            // Orderly daemon shutdown.
            *transport = None;
        }
        Ok(n) => {
            // For v1 we expect one control message per read.
            // A future iteration adds framing.
            if let Some(msg) = crate::control::parse_control(&read_buf[..n]) {
                msg.apply(&cfg.level_table);
            }
        }
        Err(TransportError::Io(e))
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            // Expected: no control traffic in this window.
        }
        Err(_) => {
            *transport = None;
        }
    }
}

/// Main flusher loop. Owns the transport handle, the current backoff,
/// and the control-message read buffer.
fn run(cfg: FlusherConfig, rx: Receiver<Vec<u8>>, stop: Arc<AtomicBool>) {
    let mut backoff = cfg.reconnect_initial_backoff;
    let mut transport: Option<Transport> = None;
    let mut read_buf = vec![0u8; 4096];

    while !stop.load(Ordering::Acquire) {
        // 1) Ensure we have an open transport. If not, reconnect.
        if transport.is_none() {
            // 1a) Connect + configure the socket. A connect-level failure
            //     (connect or set_read_timeout) sleeps for the current
            //     backoff, grows it, and retries — matching the original.
            let mut t = match try_connect(&cfg) {
                ConnectOutcome::Connected(t) => t,
                ConnectOutcome::Retry => {
                    thread::sleep(backoff);
                    backoff = (backoff * 2).min(cfg.reconnect_max_backoff);
                    continue;
                }
            };

            // 1b) Emit the drop summary (REQ_0815) on the still-pending
            //     transport, BEFORE adopting it and BEFORE resetting the
            //     backoff. A summary-write failure grows the backoff but
            //     does NOT sleep — it retries immediately on the next
            //     outer iteration. The transport is discarded (never
            //     stored) and the drop counter is left intact so the next
            //     reconnect re-attempts the summary with the same count.
            if emit_drop_summary(&mut t, &cfg).is_err() {
                backoff = (backoff * 2).min(cfg.reconnect_max_backoff);
                continue;
            }

            // 1c) Summary succeeded — adopt the transport and reset the
            //     backoff, THEN drain the ring (REQ_0814). This reset
            //     ordering is load-bearing: a mid-drain write failure
            //     (handled inside drain_ring_into_transport, which already
            //     re-buffers the remainder) leaves the backoff at its
            //     freshly-reset initial value, drops the transport, and
            //     falls through to the recv path with NO sleep and NO
            //     extra backoff growth.
            backoff = cfg.reconnect_initial_backoff;
            if drain_ring_into_transport(&mut t, &cfg).is_err() {
                transport = None;
            } else {
                transport = Some(t);
            }
        }

        // 2) Drain the producer queue with a short timeout so the loop
        //    also services the control-read path and the stop flag.
        match rx.recv_timeout(CHANNEL_RECV_TIMEOUT) {
            Ok(bytes) => handle_channel_bytes(bytes, &mut transport, &cfg),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // Idle — poll the read side for control messages.
                handle_idle(&mut transport, &cfg, &mut read_buf);
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}
