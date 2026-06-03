//! Worker pool with per-iteration barrier semantics. See design §9 (M1).

// Pool items are pub(crate) but not yet consumed until Task 7 wires them in.
#![allow(dead_code)]
// redundant_pub_crate fires because the module itself is private; the
// pub(crate) visibility is intentional for when the executor (Task 7) imports
// Pool.
#![allow(clippy::redundant_pub_crate)]

use crate::error::ExecutorError;
use crate::fatal::{FatalDispatch, FatalSite, guard_or_fatal};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

/// Unit of work submitted into the pool. Two variants:
///
/// * `Owned` carries a one-shot `Box<dyn FnOnce>` allocated by `submit`.
///   Convenient when the caller has no place to park a stable closure
///   (e.g. graph vertex dispatch where each vertex closure carries
///   per-vertex state).
/// * `Borrowed` carries a raw pointer to a `dyn FnMut() + Send` closure
///   owned by the caller. The caller guarantees the closure outlives
///   the job — discipline enforced by `pool.barrier()` before the
///   closure could be touched again. The Borrowed path performs **no
///   per-submit heap allocation**, which is required by `REQ_0060`
///   (zero-alloc steady-state dispatch).
enum Job {
    Owned(Box<dyn FnOnce() + Send + 'static>),
    Borrowed(BorrowedJob),
}

/// Send-able raw pointer to a caller-owned `FnMut` closure.
///
/// # Safety
///
/// `Send` is asserted by the pool's discipline: the caller (the
/// executor) holds exclusive access to the closure between dispatches
/// because `pool.barrier()` is called at the end of each `WaitSet`
/// callback iteration, sequencing the closure's invocation strictly
/// inside one iteration of `dispatch_loop`. The pointer is therefore
/// not aliased on the worker side at the moment a new iteration's
/// callback runs.
#[allow(unsafe_code)]
pub(crate) struct BorrowedJob(*mut (dyn FnMut() + Send));

impl BorrowedJob {
    /// Wrap a raw pointer to a caller-owned closure for the pool channel.
    ///
    /// # Safety
    ///
    /// The closure must outlive every submission of this `BorrowedJob`,
    /// and the caller must serialise submissions with `pool.barrier()`
    /// so the worker thread is not invoking it concurrently with the
    /// caller's own access.
    #[allow(unsafe_code)]
    pub(crate) const unsafe fn new(ptr: *mut (dyn FnMut() + Send)) -> Self {
        Self(ptr)
    }
}

// SAFETY: see [`BorrowedJob`] doc comment.
#[allow(unsafe_code)]
unsafe impl Send for BorrowedJob {}

/// Shared progress tracker — counts jobs submitted vs completed, used for
/// `barrier()`.
#[derive(Default)]
struct Tracker {
    submitted: AtomicUsize,
    completed: AtomicUsize,
    cv: Condvar,
    lock: Mutex<()>,
}

impl Tracker {
    fn submit(&self) {
        self.submitted.fetch_add(1, Ordering::SeqCst);
    }

    fn complete(&self) {
        self.completed.fetch_add(1, Ordering::SeqCst);
        // Acquire+drop the lock to establish happens-before with the waiter,
        // then notify *after* releasing — avoids a wake-then-sleep cycle under
        // high completion rate.
        drop(self.lock.lock().unwrap());
        self.cv.notify_all();
    }

    #[allow(clippy::significant_drop_tightening)]
    fn wait_for_quiescence(&self) {
        // The guard must be held across every cv.wait() call; clippy's
        // suggestion to drop it early would break the condvar contract.
        let mut g = self.lock.lock().unwrap();
        while self.submitted.load(Ordering::SeqCst) != self.completed.load(Ordering::SeqCst) {
            g = self.cv.wait(g).unwrap();
        }
    }
}

/// Worker pool with two modes: `n=0` runs inline; `n>=1` spawns N OS threads.
pub(crate) struct Pool {
    mode: PoolMode,
    tracker: Arc<Tracker>,
    /// Fatal-dispatch handle. Invoked from the pool worker / inline-submit
    /// panic boundaries to fail-fast on a framework-internal panic.
    pub(crate) fatal: Arc<FatalDispatch>,
}

/// Internal execution mode for the pool.
enum PoolMode {
    /// All jobs run synchronously on the calling thread.
    Inline,
    /// Jobs are dispatched to N worker threads via a bounded channel.
    Threaded {
        /// Sending end of the job channel.
        tx: Sender<Job>,
        /// Worker thread handles, drained on drop.
        handles: Vec<JoinHandle<()>>,
        /// Set to `true` to ask workers to exit after draining.
        shutdown: Arc<std::sync::atomic::AtomicBool>,
    },
}

impl Pool {
    /// Create a new pool. `n_workers == 0` selects inline mode; any positive
    /// value spawns that many OS threads. `attrs` controls thread names,
    /// CPU affinity, and scheduling priority. `fatal` is stored for use by
    /// the pool worker panic boundary (Task 3).
    pub(crate) fn new(
        n_workers: usize,
        attrs: crate::thread_attrs::ThreadAttributes,
        fatal: Arc<FatalDispatch>,
    ) -> Result<Self, ExecutorError> {
        let tracker = Arc::new(Tracker::default());
        if n_workers == 0 {
            return Ok(Self {
                mode: PoolMode::Inline,
                tracker,
                fatal,
            });
        }

        let (tx, rx): (Sender<Job>, Receiver<Job>) = bounded(n_workers * 4);
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let attrs = Arc::new(attrs);
        let mut handles = Vec::with_capacity(n_workers);
        for i in 0..n_workers {
            let rx = rx.clone();
            let tracker = Arc::clone(&tracker);
            let shutdown = Arc::clone(&shutdown);
            let attrs = Arc::clone(&attrs);
            let fatal = Arc::clone(&fatal);
            let name = {
                #[cfg(feature = "thread_attrs")]
                {
                    attrs
                        .name_prefix
                        .as_ref()
                        .map_or_else(|| format!("taktora-pool-{i}"), |p| format!("{p}-{i}"))
                }
                #[cfg(not(feature = "thread_attrs"))]
                {
                    format!("taktora-pool-{i}")
                }
            };
            let h = thread::Builder::new()
                .name(name)
                .spawn(move || {
                    attrs.apply_to_self(i);
                    while !shutdown.load(Ordering::Acquire) {
                        match rx.recv() {
                            Ok(Job::Owned(f)) => {
                                guard_or_fatal(&fatal, FatalSite::PoolWorker, f);
                                tracker.complete();
                            }
                            Ok(Job::Borrowed(b)) => {
                                // SAFETY: see BorrowedJob — caller's
                                // barrier() pairs with this invocation
                                // to ensure exclusive access.
                                #[allow(unsafe_code)]
                                guard_or_fatal(&fatal, FatalSite::PoolWorker, || unsafe {
                                    (*b.0)();
                                });
                                tracker.complete();
                            }
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|e| ExecutorError::Builder(format!("spawn worker: {e}")))?;
            handles.push(h);
        }
        Ok(Self {
            mode: PoolMode::Threaded {
                tx,
                handles,
                shutdown,
            },
            tracker,
            fatal,
        })
    }

    /// Test-only constructor injecting a specific `FatalDispatch` (e.g. one with
    /// a recording terminal) so the panic boundary can be observed without
    /// aborting the test process.
    #[cfg(test)]
    pub(crate) fn new_with_fatal(
        n_workers: usize,
        fatal: Arc<FatalDispatch>,
    ) -> Result<Self, ExecutorError> {
        Self::new(
            n_workers,
            crate::thread_attrs::ThreadAttributes::new(),
            fatal,
        )
    }

    /// Submit a job to the pool. In inline mode the job runs immediately on
    /// the calling thread; in threaded mode it is enqueued for a worker.
    ///
    /// Allocates one `Box` per call in threaded mode. For hot-path dispatch
    /// where the closure shape is stable across iterations, prefer
    /// [`Pool::submit_borrowed`] which avoids the allocation.
    #[track_caller]
    pub(crate) fn submit<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.tracker.submit();
        match &self.mode {
            PoolMode::Inline => {
                guard_or_fatal(&self.fatal, FatalSite::InlineSubmit, f);
                self.tracker.complete();
            }
            PoolMode::Threaded { tx, .. } => {
                // Safe to expect: the channel sender lives in self, and self can't be
                // dropped while we hold &self. The only path to a closed channel is
                // Pool::drop, which can't run concurrently with submit().
                tx.send(Job::Owned(Box::new(f)))
                    .expect("pool channel closed");
            }
        }
    }

    /// Submit a job whose closure is owned by the caller and remains valid
    /// across submissions. Performs **no heap allocation** per call (the
    /// closure was allocated once when the caller built it). Required by
    /// `REQ_0060`.
    ///
    /// # Safety
    ///
    /// See [`BorrowedJob::new`] — caller must hold exclusive access to the
    /// closure between submissions and pair every submit with `barrier()`
    /// before the closure could be touched again.
    #[track_caller]
    #[allow(unsafe_code)]
    pub(crate) unsafe fn submit_borrowed(&self, job: BorrowedJob) {
        self.tracker.submit();
        match &self.mode {
            PoolMode::Inline => {
                // SAFETY: caller invariant.
                guard_or_fatal(&self.fatal, FatalSite::InlineSubmit, || unsafe {
                    (*job.0)();
                });
                self.tracker.complete();
            }
            PoolMode::Threaded { tx, .. } => {
                tx.send(Job::Borrowed(job)).expect("pool channel closed");
            }
        }
    }

    /// Block until every job submitted so far has completed.
    pub(crate) fn barrier(&self) {
        self.tracker.wait_for_quiescence();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        if let PoolMode::Threaded {
            shutdown,
            handles,
            tx,
        } = &mut self.mode
        {
            shutdown.store(true, Ordering::Release);
            // Replace tx with a fresh closed channel so the original Sender is
            // dropped here. That makes recv on workers return Err(_) and lets
            // the threads exit promptly even if shutdown was checked just
            // before they entered recv().
            let (closed_tx, _) = bounded::<Job>(0);
            let _ = std::mem::replace(tx, closed_tx);
            for h in handles.drain(..) {
                let _ = h.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fatal::FatalDispatch;
    use crate::thread_attrs::ThreadAttributes;
    use std::sync::atomic::AtomicU32;

    fn noop_fatal() -> Arc<FatalDispatch> {
        Arc::new(FatalDispatch::new(Arc::new(|_| {})))
    }

    type Recorder = Arc<Mutex<Vec<(FatalSite, String)>>>;

    /// A `FatalDispatch` whose terminal records `(site, cause)` rather than
    /// aborting, so the pool's panic boundary can be observed in-process.
    fn recording_fatal() -> (Recorder, Arc<FatalDispatch>) {
        let rec: Recorder = Arc::new(Mutex::new(Vec::new()));
        let rec2 = Arc::clone(&rec);
        let dispatch = FatalDispatch::with_terminal(Arc::new(|_| {}), move |ctx| {
            rec2.lock().unwrap().push((ctx.site, ctx.cause.clone()));
        });
        (rec, Arc::new(dispatch))
    }

    #[test]
    fn inline_pool_panic_fires_fatal_with_inline_submit_site() {
        let (rec, fatal) = recording_fatal();
        let pool = Pool::new_with_fatal(0, fatal).unwrap();
        pool.submit(|| panic!("synthetic infra panic"));
        pool.barrier();
        let entries = rec.lock().unwrap().clone();
        assert_eq!(entries.len(), 1, "fatal must fire exactly once");
        assert_eq!(entries[0].0, FatalSite::InlineSubmit);
        assert_eq!(entries[0].1, "synthetic infra panic");
    }

    #[test]
    fn threaded_pool_panic_fires_fatal_with_pool_worker_site() {
        let (rec, fatal) = recording_fatal();
        let pool = Pool::new_with_fatal(2, fatal).unwrap();
        pool.submit(|| panic!("synthetic infra panic"));
        pool.barrier();
        let entries = rec.lock().unwrap().clone();
        assert_eq!(entries.len(), 1, "fatal must fire exactly once");
        assert_eq!(entries[0].0, FatalSite::PoolWorker);
        assert_eq!(entries[0].1, "synthetic infra panic");
    }

    // ── TEST_0824 — subprocess SIGABRT ────────────────────────────────────────

    /// Env-var guarded child branch: build a *real* abort-terminal pool and
    /// drive a panicking job through the framework boundary, which must
    /// `std::process::abort()` (SIGABRT).
    #[cfg(unix)]
    const ABORT_CHILD_ENV: &str = "TAKTORA_EXECUTOR_ABORT_CHILD";

    #[cfg(unix)]
    #[test]
    fn pool_panic_aborts_process_with_sigabrt() {
        use std::os::unix::process::ExitStatusExt;

        if std::env::var(ABORT_CHILD_ENV).is_ok() {
            // --- child branch ---
            // Real abort-terminal dispatch. Submitting a panicking job through
            // the boundary must abort the process.
            let fatal = Arc::new(FatalDispatch::new(Arc::new(|_| {})));
            let pool = Pool::new(2, ThreadAttributes::new(), fatal).unwrap();
            pool.submit(|| panic!("synthetic infra panic in child"));
            pool.barrier();
            // Should never get here — the worker boundary aborted.
            std::process::exit(0);
        }

        // --- parent branch ---
        // Bounded wait: spawn the child and poll `try_wait` so a misbehaving
        // child that fails to abort fails this test loudly instead of hanging
        // CI forever. The child's stdout/stderr are nulled — it deliberately
        // panics and aborts, and that noise ("Aborted", panic message) would
        // otherwise pollute CI logs.
        let poll_interval = std::time::Duration::from_millis(50);
        let max_wait = std::time::Duration::from_secs(30);

        let exe = std::env::current_exe().expect("current_exe");
        let mut child = std::process::Command::new(exe)
            .args([
                "--exact",
                "pool::tests::pool_panic_aborts_process_with_sigabrt",
            ])
            .env(ABORT_CHILD_ENV, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn child test process");

        let deadline = std::time::Instant::now() + max_wait;

        let status = loop {
            if let Some(status) = child.try_wait().expect("try_wait on child test process") {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                // Best-effort kill so we don't leak the process, then fail
                // loudly rather than hang the suite.
                let _ = child.kill();
                panic!("child did not abort within 30s");
            }
            std::thread::sleep(poll_interval);
        };

        assert_eq!(
            status.signal(),
            Some(6),
            "child must die via SIGABRT (signal 6); status: {status:?}"
        );
    }

    #[test]
    fn inline_pool_runs_synchronously() {
        let pool = Pool::new(0, ThreadAttributes::new(), noop_fatal()).unwrap();
        let counter = Arc::new(AtomicU32::new(0));
        for _ in 0..10 {
            let c = Arc::clone(&counter);
            pool.submit(move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        }
        pool.barrier();
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn threaded_pool_runs_concurrently_and_barriers() {
        let pool = Pool::new(4, ThreadAttributes::new(), noop_fatal()).unwrap();
        let counter = Arc::new(AtomicU32::new(0));
        for _ in 0..100 {
            let c = Arc::clone(&counter);
            pool.submit(move || {
                std::thread::sleep(std::time::Duration::from_millis(1));
                c.fetch_add(1, Ordering::SeqCst);
            });
        }
        pool.barrier();
        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn barrier_with_no_work_returns_immediately() {
        let pool = Pool::new(2, ThreadAttributes::new(), noop_fatal()).unwrap();
        pool.barrier();
        // No assertion — must not deadlock.
    }

    #[test]
    fn submitted_panic_fires_fatal_and_completion_is_counted() {
        // A framework-boundary panic now fails fast (fires the fatal dispatch).
        // Under a recording terminal (instead of abort) we can observe that the
        // fatal fired AND that `tracker.complete()` still ran afterward, so the
        // barrier does not hang. A regression of "tracker.complete() skipped on
        // the fatal path" would surface here as a 60s hang / counter mismatch.
        let (rec, fatal) = recording_fatal();
        let pool = Pool::new_with_fatal(2, fatal).unwrap();
        pool.submit(|| panic!("kaboom"));
        pool.submit(|| {});
        pool.barrier();
        assert_eq!(
            pool.tracker.submitted.load(Ordering::SeqCst),
            pool.tracker.completed.load(Ordering::SeqCst),
            "submitted vs completed counters diverged after panic"
        );
        let entries = rec.lock().unwrap().clone();
        assert_eq!(entries.len(), 1, "exactly one fatal should have fired");
        assert_eq!(entries[0].0, FatalSite::PoolWorker);
        assert_eq!(entries[0].1, "kaboom");
    }
}
