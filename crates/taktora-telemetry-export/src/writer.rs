//! Drain thread: pops [`PodRecord`]s off the ring and writes NDJSON to a sink,
//! off the RT thread.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::ring::{Consumer, RecvOutcome};

/// Totals reported when the drain thread finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrainSummary {
    /// Records written to the sink.
    pub written: u64,
    /// Records lost to overwrite because the drain fell behind.
    pub lapped: u64,
}

/// Handle to a running drain thread. Call [`finish`](Self::finish) to stop it,
/// flush the sink, and collect the [`DrainSummary`].
///
/// Always stop the writer via [`finish`](Self::finish). Dropping an
/// `NdjsonWriter` without calling it leaves the drain thread running until the
/// process exits (the stop flag is never set), leaking the thread and its sink.
pub struct NdjsonWriter {
    handle: JoinHandle<io::Result<DrainSummary>>,
    stop: Arc<AtomicBool>,
}

impl NdjsonWriter {
    /// Signal the drain thread to finish (after one final drain pass), join it,
    /// flush the sink, and return the summary.
    ///
    /// # Preconditions
    ///
    /// Stop the producer (drop or quiesce it) before calling `finish`. The
    /// drain thread exits on the first `Empty` it observes after the stop flag
    /// is set, so records pushed concurrently with `finish` may be lost.
    ///
    /// # Panics
    ///
    /// Panics if the drain thread panicked.
    pub fn finish(self) -> io::Result<DrainSummary> {
        self.stop.store(true, Ordering::Release);
        self.handle.join().expect("drain thread panicked")
    }
}

/// Spawn a drain thread that writes every ring record to `sink` as NDJSON.
///
/// The thread runs until [`NdjsonWriter::finish`] is called, then performs a
/// final drain pass so records published just before shutdown are not lost.
pub fn spawn<W: Write + Send + 'static>(mut consumer: Consumer, mut sink: W) -> NdjsonWriter {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let handle = thread::spawn(move || -> io::Result<DrainSummary> {
        let mut written = 0u64;
        let mut lapped = 0u64;
        loop {
            match consumer.try_recv() {
                RecvOutcome::Record(rec) => {
                    rec.write_ndjson(&mut sink)?;
                    written += 1;
                }
                RecvOutcome::Lapped { skipped } => lapped += skipped,
                RecvOutcome::Empty => {
                    if stop_thread.load(Ordering::Acquire) {
                        // Final pass: the producer has stopped, so `Empty` here
                        // means fully drained.
                        break;
                    }
                    // Idle a beat so we do not spin a core while waiting.
                    thread::sleep(Duration::from_micros(200));
                }
            }
        }
        sink.flush()?;
        Ok(DrainSummary { written, lapped })
    });

    NdjsonWriter { handle, stop }
}
