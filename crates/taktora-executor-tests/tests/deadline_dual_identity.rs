//! Issue #94 — end-to-end proof of the deadline attachment's *dual identity*.
//!
//! A deadline trigger attaches ONE WaitSet guard, yet that guard fires under
//! two distinct attachment-id forms that the `AttachmentMap` must both route to
//! the SAME task:
//!
//!   * **Real event (Notification-form id):** when the paired subscriber
//!     actually receives a sample, the guard fires as a Notification-form id.
//!     That id's constructor is private upstream, so it CANNOT be precomputed
//!     at build time — the map learns it lazily on first fire (one-shot linear
//!     scan) and then caches it.
//!   * **Missed deadline (Deadline-form id):** when no event arrives within the
//!     deadline window, the SAME guard fires as the Deadline-form id, which IS
//!     precomputed at build time (`from_guard` on the deadline guard yields the
//!     Deadline-form) → a binary-search HIT.
//!
//! The unit tests in `taktora-executor` exercise each `AttachmentMap` path in
//! isolation but cannot observe that BOTH id forms, fired against a live
//! WaitSet by a real executor, resolve to the same registered task. This
//! integration test closes that gap: it registers ONE deadline-triggered task,
//! drives the attachment to fire first as a real event and then as a missed
//! deadline, and asserts the same task body dispatches in both phases.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use iceoryx2::prelude::*;
use taktora_executor::{Executor, ExecutorError, ItemFlow, item_with_triggers};

/// Deadline window. Comfortably larger than the time the WaitSet needs to wake
/// on a pending event (phase A) so the real-event path never accidentally trips
/// the deadline, yet small enough to keep the missed-deadline phase (phase B)
/// quick. Wall-clock dependent only in phase B.
const DEADLINE_MS: u64 = 150;

/// Per-run unique topic name (`{prefix}.{pid}.{seq}`). iceoryx2 services persist
/// beyond the process and `open_or_create` *attaches* to a pre-existing service,
/// so a static topic would let a concurrent run of this binary publish onto our
/// listener and break the phase-A-vs-phase-B counter. Mirrors the workspace
/// convention (see `taktora-executor/tests/channel.rs`).
static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn unique(prefix: &str) -> String {
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{prefix}.{}.{n}", std::process::id())
}

#[derive(Debug, Default, Clone, Copy, ZeroCopySend)]
#[repr(C)]
struct Msg(u32);

#[test]
fn deadline_dual_identity_routes_both_id_forms_to_same_task() {
    let mut exec = Executor::builder()
        .worker_threads(0) // inline dispatch — deterministic, no pool jitter
        .build()
        .expect("build executor");

    let node = NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create node");
    let ch =
        taktora_executor::Channel::<Msg>::open_or_create(&node, &unique("taktora.test.dualid"))
            .expect("open_or_create channel");
    let sub = ch.subscriber().expect("subscriber");
    let publisher = ch.publisher().expect("publisher");

    // Dispatch count lives in the task body: `on_cycle_stats` does NOT fire for
    // event-driven tasks (`record_cycle_for` early-returns when `scan_period`
    // is `None`, which is the case for deadline/subscriber tasks), so an
    // Observer would never see these dispatches. Count inside the body instead.
    let dispatches = Arc::new(AtomicUsize::new(0));
    let dispatches_for_item = Arc::clone(&dispatches);

    // Retain a clone of the deadline listener handle so the TEST THREAD can
    // drain it between phases. iceoryx2's WaitSet is level-triggered on the
    // listener fd: a `notify()` (phase A's `send_copy`) stays queued until the
    // application drains it via `try_wait_one`. The executor's dispatch path
    // does NOT drain subscriber listeners (only the stop listener), so without
    // this drain the phase-A notification would keep the fd ready and re-fire as
    // an *event* in phase B — never reaching the missed-deadline path.
    //
    // The handle is `!Send` (the inner listener holds an `Rc`), so it cannot be
    // captured by the executor's `Send`-bounded task body. Draining it from the
    // test thread is sound here: `worker_threads(0)` runs dispatch INLINE and
    // `run_n` blocks the caller until its iteration completes, so the executor
    // never touches the listener concurrently with the drain calls below — they
    // happen strictly between the two `run_n` calls, on one thread.
    let listener = sub.listener_handle();

    // `sub` is moved into the declare closure: `item_with_triggers` requires
    // the declarer be `Send`, and a borrow of `Subscriber` is not `Send`
    // (`Subscriber` is move-only, never shared). The publisher is independent of
    // the subscriber, so it stays out here to drive phase A.
    exec.add(item_with_triggers(
        move |d| -> Result<(), ExecutorError> {
            d.deadline(&sub, Duration::from_millis(DEADLINE_MS));
            Ok(())
        },
        move |_ctx| {
            dispatches_for_item.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .expect("add deadline-triggered task");

    // ---- Phase A: real event → Notification-form id (lazy-learned) ----------
    //
    // A pending sample makes the listener fd ready, so the WaitSet wakes on the
    // event near-instantly (well under DEADLINE_MS) and dispatches the task.
    // The Notification-form id is unknown to the map, so `resolve` falls back to
    // a one-shot `linear_scan`, learns it, caches it, and dispatches.
    publisher.send_copy(Msg(1)).expect("publish real event");
    exec.run_n(1).expect("run phase A (real event)");
    assert_eq!(
        dispatches.load(Ordering::SeqCst),
        1,
        "phase A: a real event (Notification-form id, lazy-learned) must \
         dispatch the deadline task exactly once",
    );

    // Drain the phase-A notification so the listener fd is no longer ready.
    // Without this, the un-consumed `notify()` would re-fire as an *event* in
    // phase B and the deadline would never elapse (see the comment on
    // `listener`). Safe to call here: single-threaded, strictly between the two
    // `run_n` calls.
    while let Ok(Some(_)) = listener.try_wait_one() {}

    // ---- Phase B: missed deadline → Deadline-form id (precomputed hit) ------
    //
    // No event is published, so the WaitSet blocks ~DEADLINE_MS, then wakes on
    // the missed deadline. The deadline guard's `from_guard` id is the
    // Deadline-form, precomputed at build time → an `AttachmentMap`
    // binary-search HIT that resolves to the SAME task as phase A.
    exec.run_n(1).expect("run phase B (missed deadline)");
    assert_eq!(
        dispatches.load(Ordering::SeqCst),
        2,
        "phase B: a missed deadline (Deadline-form id, precomputed binary-search \
         hit) must route to the SAME single task as the phase-A real event — the \
         two dispatches on one task prove both id forms of the deadline \
         attachment's dual identity resolve identically",
    );
}
