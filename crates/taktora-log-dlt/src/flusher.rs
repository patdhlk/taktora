//! Background flusher: owns the socket, drains the producer channel
//! and the offline ring, applies control messages.
//!
//! REQ_0812: producer does not block — the flusher does the I/O.
//! REQ_0814: the offline ring is drained FIFO on reconnect.
//! REQ_0815: the drop-summary record (synthesised by the caller from
//! [`OfflineRing::drops_since_last_drain`]) is emitted at the head of
//! the drain after an overflow event. The flusher itself does not
//! synthesise that record — Task 17 (the public [`log::Log`] adapter)
//! is responsible for prepending it to the producer queue before any
//! buffered records.
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

/// Main flusher loop. Owns the transport handle, the current backoff,
/// and the control-message read buffer.
fn run(cfg: FlusherConfig, rx: Receiver<Vec<u8>>, stop: Arc<AtomicBool>) {
    let mut backoff = cfg.reconnect_initial_backoff;
    let mut transport: Option<Transport> = None;
    let mut read_buf = vec![0u8; 4096];

    while !stop.load(Ordering::Acquire) {
        // 1) Ensure we have an open transport. If not, reconnect.
        if transport.is_none() {
            match Transport::connect(&cfg.transport) {
                Ok(mut t) => {
                    // Best-effort: a missing read timeout just means the
                    // control-poll path may block until daemon sends data.
                    // Production daemons send heartbeats; tests don't, so
                    // failure to set this would still hang the test.
                    if t.set_read_timeout(SOCKET_READ_TIMEOUT).is_err() {
                        // If we can't even configure the socket, drop it and
                        // try again on the next iteration.
                        thread::sleep(backoff);
                        backoff = (backoff * 2).min(cfg.reconnect_max_backoff);
                        continue;
                    }
                    transport = Some(t);
                    backoff = cfg.reconnect_initial_backoff;

                    // On reconnect, drain the offline ring FIFO (REQ_0814).
                    if let Some(t) = transport.as_mut() {
                        let drained = cfg.ring.drain_all();
                        let mut iter = drained.into_iter();
                        while let Some(bytes) = iter.next() {
                            if t.write_all(&bytes).is_err() {
                                // Re-buffer the failed record first, then every
                                // remaining undrained item — preserves FIFO across
                                // the reconnect attempt. Note: concurrent producers
                                // that pushed to the ring during the drain window
                                // sit AHEAD of this re-buffered remainder on the
                                // next drain. That ordering blemish is acceptable
                                // for v1 — strict cross-reconnect FIFO is not
                                // required by the spec (REQ_0814).
                                cfg.ring.push(bytes);
                                for rest in iter.by_ref() {
                                    cfg.ring.push(rest);
                                }
                                transport = None;
                                break;
                            }
                        }
                    }
                }
                Err(_) => {
                    thread::sleep(backoff);
                    backoff = (backoff * 2).min(cfg.reconnect_max_backoff);
                    continue;
                }
            }
        }

        // 2) Drain the producer queue with a short timeout so the loop
        //    also services the control-read path and the stop flag.
        match rx.recv_timeout(CHANNEL_RECV_TIMEOUT) {
            Ok(bytes) => {
                if let Some(t) = transport.as_mut() {
                    if t.write_all(&bytes).is_err() {
                        // Daemon dropped — buffer this record and reconnect.
                        cfg.ring.push(bytes);
                        transport = None;
                    }
                } else {
                    cfg.ring.push(bytes);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // Idle — poll the read side for control messages.
                if let Some(t) = transport.as_mut() {
                    match t.read(&mut read_buf) {
                        Ok(0) => {
                            // Orderly daemon shutdown.
                            transport = None;
                        }
                        Ok(n) => {
                            // For v1 we expect one control message per
                            // read. A future iteration adds framing.
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
                            transport = None;
                        }
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}
