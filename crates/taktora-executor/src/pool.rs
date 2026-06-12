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
use std::sync::Arc;
use std::thread::{self, JoinHandle};

// `Tracker`'s sync primitives are aliased so its quiescence handshake can be
// model-checked under `--cfg loom` (see the `loom_tests` module). Only the
// handshake is modelled; the rest of this module (Pool, run_worker) stays
// std-only and is never exercised inside `loom::model`.
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering, fence};
#[cfg(loom)]
use loom::sync::{Condvar, Mutex};
#[cfg(not(loom))]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering, fence};
#[cfg(not(loom))]
use std::sync::{Condvar, Mutex};

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
///
/// # Quiescence handshake
///
/// In steady state `complete()` runs once per pool job, so it must be cheap.
/// The naive design (`notify_all` on every completion) costs one `futex_wake`
/// syscall per job even when no thread is parked — O(N) wasted syscalls per
/// cycle. Instead, the single waiting thread publishes a `waiting` flag before
/// it parks; `complete()` only takes the lock and notifies when the flag is
/// set, taking the steady-state cost to O(1) wakes per cycle (and zero in
/// inline mode, where nothing ever parks).
///
/// Correctness rests on **two orthogonal mechanisms** — do not conflate them:
///
/// * **Flag-decision race (Dekker).** `complete()` increments `completed` then
///   reads `waiting`; the waiter stores `waiting = true` then re-reads
///   `completed`. That is a store-then-load of *different* locations on each
///   thread, and the lock does **not** serialise it because `complete()` reads
///   `waiting` lock-free. A `StoreLoad` reordering would let both threads read
///   stale values — the completer skips the notify *and* the waiter parks on a
///   stale count → a permanent hang. The fix is a `SeqCst` **fence** between
///   the store and the load on each side: the two fences sit in one total
///   order, so whichever fence is first, its thread's store is visible to the
///   other thread's post-fence load. Plain `SeqCst` *atomics* would also be
///   correct on hardware, but loom (the verifier) models `SeqCst` atomics only
///   as `AcqRel` and cannot prove the cross-location order — it *can* model
///   `SeqCst` fences, so the fence formulation is what makes `loom_tests`
///   mechanical rather than a hand proof. Acquire/Release (no fence) is **not**
///   sufficient.
/// * **Park-handshake race.** Even once `complete()` decides to notify, firing
///   between the waiter's final re-check and its `cv.wait()` would be lost. The
///   lock-acquire/drop in `complete()` closes that window (the waiter holds the
///   lock across the check; `cv.wait()` releases it atomically).
struct Tracker {
    submitted: AtomicUsize,
    completed: AtomicUsize,
    /// Set by the (single) waiter before it parks; gates `complete()`'s notify.
    waiting: AtomicBool,
    cv: Condvar,
    lock: Mutex<()>,
}

impl Tracker {
    // Not `const`: under `--cfg loom` the sync primitives are `loom::sync` types
    // whose `::new` constructors are not `const`. clippy only sees the std path.
    #[allow(clippy::missing_const_for_fn)]
    fn new() -> Self {
        Self {
            submitted: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            waiting: AtomicBool::new(false),
            cv: Condvar::new(),
            lock: Mutex::new(()),
        }
    }

    fn submit(&self) {
        self.submitted.fetch_add(1, Ordering::SeqCst);
    }

    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn complete(&self) {
        // Release so the waiter that observes this increment also sees the
        // completed job's writes (the barrier's memory-visibility guarantee).
        self.completed.fetch_add(1, Ordering::Release);
        // `StoreLoad` fence: orders the increment above before the `waiting` load
        // below. Paired with the waiter's fence, the two SeqCst fences sit in a
        // single total order, so the threads cannot both miss each other — see
        // the `Tracker` doc comment (the wakeup Dekker).
        fence(Ordering::SeqCst);
        if self.waiting.load(Ordering::Relaxed) {
            // Acquire+drop the lock to establish happens-before with the
            // waiter, then notify *after* releasing — closes the park-handshake
            // window. notify_one suffices: there is exactly one waiter.
            #[allow(clippy::unwrap_used)]
            // fail-fast: mutex poison is unreachable under the abort boundary (ADR_0065)
            let guard = self.lock.lock().unwrap();
            drop(guard); // release BEFORE notifying (see comment above)
            self.cv.notify_one();
        }
    }

    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(clippy::significant_drop_tightening)]
    fn wait_for_quiescence(&self) {
        // The guard must be held across every cv.wait() call; clippy's
        // suggestion to drop it early would break the condvar contract.
        // fail-fast: mutex poison is unreachable under the abort boundary (ADR_0065)
        #[allow(clippy::unwrap_used)]
        let mut g = self.lock.lock().unwrap();
        // Publish that we are about to park *before* the re-check, so any
        // completer that increments `completed` after this point is guaranteed
        // (via SeqCst) to observe the flag and notify. `swap` lets us assert the
        // single-waiter invariant that `notify_one` depends on.
        let prev = self.waiting.swap(true, Ordering::Relaxed);
        debug_assert!(
            !prev,
            "Tracker supports a single waiter; concurrent barrier() detected"
        );
        // `StoreLoad` fence: orders the `waiting` store above before the
        // `completed` load below, pairing with the completer's fence (see the
        // `Tracker` doc comment). Acquire on the `completed` load receives the
        // completer's Release increment (memory-visibility on quiescence).
        fence(Ordering::SeqCst);
        while self.submitted.load(Ordering::Relaxed) != self.completed.load(Ordering::Acquire) {
            // fail-fast: mutex poison is unreachable under the abort boundary (ADR_0065)
            #[allow(clippy::unwrap_used)]
            let next = self.cv.wait(g).unwrap();
            g = next;
        }
        // Reset under the lock before dropping the guard. A completer may still
        // observe a stale `true` and fire one wasted `notify_one`; that is
        // harmless (it wakes nobody, or spuriously wakes the next cycle's waiter
        // which re-checks the loop condition).
        self.waiting.store(false, Ordering::Relaxed);
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

/// Cyclic worker-loop body — processes jobs from `rx` until `shutdown` is
/// set or the channel is closed. Extracted from the closure inside
/// [`Pool::new`] so the no-panic classification gate can be applied to
/// exactly this function (not to `Pool::new` build-time setup code).
///
/// The `#[deny(...)]` below ensures every `unwrap`/`expect`/`panic!` on
/// this hot path is an explicitly classified fail-fast; any new unclassified
/// site will be a compile error.
#[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Each Arc is an owned clone given to the spawned thread; pass-by-value
// is intentional — the thread takes ownership of its share.
#[allow(clippy::needless_pass_by_value)]
#[allow(unsafe_code)]
fn run_worker(
    rx: Receiver<Job>,
    tracker: Arc<Tracker>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    fatal: Arc<FatalDispatch>,
) {
    while !shutdown.load(Ordering::Acquire) {
        match rx.recv() {
            Ok(Job::Owned(f)) => {
                guard_or_fatal(&fatal, FatalSite::PoolWorker, f);
                tracker.complete();
            }
            Ok(Job::Borrowed(b)) => {
                // SAFETY: see BorrowedJob — caller's barrier() pairs with
                // this invocation to ensure exclusive access.
                guard_or_fatal(&fatal, FatalSite::PoolWorker, || unsafe {
                    (*b.0)();
                });
                tracker.complete();
            }
            Err(_) => break,
        }
    }
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
        let tracker = Arc::new(Tracker::new());
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
                    run_worker(rx, tracker, shutdown, fatal);
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
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
                // fail-fast: pool channel only closes in Pool::drop, which
                // cannot run concurrently with submit
                #[allow(clippy::expect_used)]
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
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
                // fail-fast: pool channel only closes in Pool::drop, which
                // cannot run concurrently with dispatch
                #[allow(clippy::expect_used)]
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

// Std-thread functional tests — not built under `--cfg loom` (they spawn real
// OS threads and a subprocess, which loom must not intercept).
#[cfg(all(test, not(loom)))]
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

// Loom model of the `Tracker` quiescence handshake. Run with:
//   RUSTFLAGS="--cfg loom" cargo test -p taktora-executor --lib loom_tests
//
// Loom exhaustively explores the thread interleavings and memory-ordering
// reorderings permitted by the C11 model. It is the regression gate for the
// Dekker race described on `Tracker`: with anything weaker than `SeqCst` on the
// `waiting`/`completed` handshake, loom finds the interleaving where the waiter
// parks forever (reported as a deadlock).
#[cfg(all(test, loom))]
mod loom_tests {
    use super::Tracker;
    use loom::sync::Arc;
    use loom::sync::atomic::Ordering;

    /// One waiter racing one completer (`submitted == 1`). This is the minimal
    /// model that exercises the flag-decision race: the waiter may publish
    /// `waiting` and re-check `completed` concurrently with the completer
    /// incrementing `completed` and reading `waiting`. The barrier must always
    /// return — if it can't, loom reports the parked waiter as a deadlock.
    #[test]
    fn barrier_never_misses_wakeup_single_completer() {
        loom::model(|| {
            let tracker = Arc::new(Tracker::new());
            tracker.submit();

            let completer = {
                let tracker = Arc::clone(&tracker);
                loom::thread::spawn(move || {
                    tracker.complete();
                })
            };

            // The model thread is the single waiter. Reaching the line after
            // this call means no wakeup was lost.
            tracker.wait_for_quiescence();
            completer.join().unwrap();

            assert_eq!(
                tracker.submitted.load(Ordering::SeqCst),
                tracker.completed.load(Ordering::SeqCst),
                "barrier returned before quiescence",
            );
        });
    }

    /// One waiter racing two completers (`submitted == 2`). Widens the
    /// interleaving space so the race is exercised when the waiter parks after
    /// one of two completions and must still be woken by the second.
    ///
    /// Bounded with a preemption limit: a full exhaustive run with three
    /// threads and a condvar is ~minutes. The single-completer test above runs
    /// unbounded (it is the primary Dekker gate and caught every wrong
    /// ordering); this one trades exhaustiveness for CI-tractable coverage of
    /// the multi-completer shape.
    #[test]
    fn barrier_never_misses_wakeup_two_completers() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(3);
        builder.check(|| {
            let tracker = Arc::new(Tracker::new());
            tracker.submit();
            tracker.submit();

            let completers: Vec<_> = (0..2)
                .map(|_| {
                    let tracker = Arc::clone(&tracker);
                    loom::thread::spawn(move || {
                        tracker.complete();
                    })
                })
                .collect();

            tracker.wait_for_quiescence();
            for c in completers {
                c.join().unwrap();
            }

            assert_eq!(
                tracker.submitted.load(Ordering::SeqCst),
                tracker.completed.load(Ordering::SeqCst),
                "barrier returned before quiescence",
            );
        });
    }
}
