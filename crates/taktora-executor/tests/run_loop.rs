#![allow(missing_docs)]
use core::time::Duration;
use iceoryx2::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use taktora_executor::{ControlFlow, Executor, ExecutorError, TriggerDeclarer, item_with_triggers};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique(prefix: &str) -> String {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}.{}.{n}", std::process::id())
}

#[test]
fn interval_trigger_fires_run_n_times() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();
    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ControlFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(3).unwrap();

    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[test]
fn run_for_terminates_on_timeout() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();
    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(50));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ControlFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_for(Duration::from_millis(120)).unwrap();
    assert!(counter.load(Ordering::SeqCst) >= 1);
}

#[test]
fn stoppable_terminates_run() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();
    let stop = exec.stoppable();
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |ctx| {
            ctx.stop_executor();
            Ok(ControlFlow::Continue)
        },
    ))
    .unwrap();
    exec.run().unwrap();
    let _ = stop;
}

#[derive(Debug, Default, Clone, Copy, ZeroCopySend)]
#[repr(C)]
struct Tick(u64);

#[test]
fn subscriber_trigger_dispatches_task() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();
    let topic = unique("taktora.test.run.sub");
    let ch = exec.channel::<Tick>(&topic).unwrap();
    let publisher = ch.publisher().unwrap();
    let subscriber = ch.subscriber().unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);
    let stop = exec.stoppable();

    exec.add(item_with_triggers(
        move |d| {
            d.subscriber(&subscriber);
            Ok(())
        },
        move |ctx| {
            c.fetch_add(1, Ordering::SeqCst);
            if c.load(Ordering::SeqCst) >= 3 {
                ctx.stop_executor();
            }
            Ok(ControlFlow::Continue)
        },
    ))
    .unwrap();

    std::thread::spawn(move || {
        for i in 0..5 {
            let _ = publisher.send_copy(Tick(i));
            std::thread::sleep(Duration::from_millis(20));
        }
    });

    exec.run().unwrap();
    let _ = stop;
    assert!(counter.load(Ordering::SeqCst) >= 3);
}

#[test]
fn threaded_pool_executes_items_correctly() {
    // Exercises the pool barrier + SendItemPtr discipline. With
    // worker_threads(2) the run loop dispatches each fired interval
    // trigger to a pool worker, then barriers before re-attaching.
    let mut exec = Executor::builder().worker_threads(2).build().unwrap();
    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ControlFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(5).unwrap();
    assert_eq!(
        counter.load(Ordering::SeqCst),
        5,
        "threaded pool should fire item exactly 5 times under run_n(5)"
    );
}

#[test]
fn item_task_id_override_takes_precedence() {
    use taktora_executor::{Context, ExecuteResult};

    struct NamedItem;
    impl taktora_executor::ExecutableItem for NamedItem {
        fn declare_triggers(&mut self, d: &mut TriggerDeclarer<'_>) -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(20));
            Ok(())
        }
        fn execute(&mut self, ctx: &mut Context<'_>) -> ExecuteResult {
            ctx.stop_executor();
            Ok(ControlFlow::Continue)
        }
        fn task_id(&self) -> Option<&str> {
            Some("custom-from-item")
        }
    }

    let mut exec = Executor::builder().worker_threads(0).build().unwrap();
    let id = exec.add_with_id("user-supplied-id", NamedItem).unwrap();
    assert_eq!(
        id.as_str(),
        "custom-from-item",
        "ExecutableItem::task_id() override should win over user-supplied id"
    );
    exec.run().unwrap();
}

/// A bare `EINTR` (any handled signal — job control, debugger attach, a
/// user-installed handler) must NOT terminate the run loop: only
/// SIGINT/SIGTERM (iceoryx2 `TerminationRequest`) and `stop()` may end it.
/// Regression test for the silent-truncation bug found on the Pi5 rig:
/// `kill -STOP`/`-CONT` ended `run_n(60000)` cleanly at ~5k cycles, exit 0.
#[cfg(unix)]
#[test]
#[allow(unsafe_code)]
fn run_n_survives_eintr_from_unrelated_signals() {
    use std::sync::mpsc;

    // No-op handler for SIGUSR1, registered WITHOUT `SA_RESTART` so a
    // blocked `epoll_wait`/`select` returns `EINTR` (`epoll_wait` is never
    // auto-restarted anyway, see signal(7)).
    extern "C" fn noop(_: libc::c_int) {}
    // SAFETY: installs a trivially async-signal-safe no-op handler for a
    // signal this test owns; no other test in this binary touches SIGUSR1.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = noop as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        assert_eq!(
            libc::sigaction(libc::SIGUSR1, &sa, std::ptr::null_mut()),
            0,
            "sigaction(SIGUSR1) failed"
        );
    }

    let mut exec = Executor::builder().worker_threads(0).build().unwrap();
    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(5));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ControlFlow::Continue)
        },
    ))
    .unwrap();

    // Run the dispatch loop on its own thread and ferry out its pthread id
    // so the signals land exactly on the thread blocked in the WaitSet.
    let (tid_tx, tid_rx) = mpsc::channel();
    let dispatch = std::thread::spawn(move || {
        // SAFETY: pthread_self is always safe to call.
        let tid = unsafe { libc::pthread_self() };
        tid_tx.send(tid as usize).expect("send tid");
        exec.run_n(50).map(|()| exec)
    });
    let tid = tid_rx.recv().expect("recv tid") as libc::pthread_t;

    // Pelt the dispatch thread with EINTRs for the whole nominal run
    // window (50 cycles x 5 ms = 250 ms).
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(10));
        // SAFETY: tid stays valid — the thread is joined below, after the
        // signal loop completes.
        unsafe {
            libc::pthread_kill(tid, libc::SIGUSR1);
        }
    }

    let result = dispatch.join().expect("dispatch thread panicked");
    assert!(result.is_ok(), "run_n failed: {:?}", result.err());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        50,
        "EINTR must not truncate run_n: all 50 cycles must dispatch"
    );
}
