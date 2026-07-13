//! The dispatch thread must run with tight (1 µs) timer slack
//! (`REQ_0274` / `TEST_0855`).
//!
//! `SCHED_OTHER` threads inherit the kernel's 50 µs default timer slack,
//! and `epoll_wait` sleeps through `schedule_hrtimeout_range` WITH that
//! slack — measured on the Pi5 rig as 56 µs/cycle accumulated drift in
//! `Legacy` dispatch (vs 5.5 µs/cycle at 1 µs slack; the residual is
//! iceoryx2's ms-rounded epoll timeout + wake latency). Real-time classes
//! force slack to 0, which is why the same rig under `SCHED_FIFO` only
//! drifted ~3 µs/cycle. The executor must not leave a 10× cyclic-precision
//! regression on the table for non-RT deployments.

// Linux-only: timer slack is a Linux facility (prctl PR_SET_TIMERSLACK,
// /proc/thread-self/timerslack_ns).
#![cfg(target_os = "linux")]
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use taktora_executor::{Executor, ItemFlow, item_with_triggers};

#[test]
fn dispatch_thread_runs_with_one_microsecond_timer_slack() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();

    // worker_threads(0): the body runs ON the dispatch thread, so
    // /proc/thread-self is the thread whose slack gates the WaitSet sleeps.
    let slack = Arc::new(AtomicU64::new(0));
    let s = Arc::clone(&slack);
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |_| {
            // timerslack_ns only exists at the top /proc/<pid>/ level
            // (proc(5)), and /proc/<tid>/ resolves for any thread — it is
            // NOT mirrored under /proc/self/task/<tid>/ on the Pi's 6.12
            // kernel. SAFETY: gettid has no preconditions.
            #[allow(unsafe_code)]
            let tid = unsafe { libc::gettid() };
            let raw = std::fs::read_to_string(format!("/proc/{tid}/timerslack_ns"))
                .expect("read timerslack_ns");
            s.store(
                raw.trim().parse().expect("parse timerslack_ns"),
                Ordering::SeqCst,
            );
            Ok(ItemFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(1).unwrap();

    assert_eq!(
        slack.load(Ordering::SeqCst),
        1_000,
        "dispatch thread must run with 1 µs timer slack (kernel default 50 µs \
         costs ~50 µs/cycle Legacy drift under SCHED_OTHER)"
    );
}
