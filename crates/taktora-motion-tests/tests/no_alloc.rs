//! REQ_0864 envelope for the NC spine: a warm steady-state `NcCycle::step`
//! (no fault, empty commands) performs **zero** heap allocations. Mirrors the
//! motion-core `CountingAllocator` harness (the TEST_0170 pattern) — a global
//! counting allocator with a tracking window, differential `big - small`
//! measurement to cancel one-time setup, single-threaded so nothing else
//! perturbs the count.
//!
//! The nominal `step` path is allocation-free by construction (fixed-size
//! per-axis buffers; the only `Vec`-returning call, `topology::engaged_downstream`,
//! is gated behind the fault path and never reached when no axis faults). This
//! test is the regression guard that keeps it so.
//!
//! `step` is `async`, so the measured region drives the future to completion
//! with a hand-rolled, allocation-free poll loop over a no-op `RawWaker`
//! (mirroring `taktora-cyclic-fieldbus`'s test `block_on`). `pollster::block_on`
//! is **not** used inside the measured window — its parker may allocate per
//! call, and the differential cancels one-time setup, not per-call overhead.

#![allow(clippy::doc_markdown)]

use core::future::Future;

use taktora_bounded_alloc::CountingAllocator;
use taktora_motion::cycle::NcCycle;
use taktora_motion::mock::MockCyclicFieldbus;
use taktora_motion::scale::AxisScale;

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();

const N: usize = 4;
const DT: f64 = 0.002;
const TICKS_SMALL: usize = 1_000;
const TICKS_BIG: usize = 10_000;

fn count_allocs<R>(f: impl FnOnce() -> R) -> (usize, R) {
    ALLOC.reset();
    ALLOC.set_tracking(true);
    let r = f();
    ALLOC.set_tracking(false);
    (ALLOC.alloc_count(), r)
}

/// Drive an immediately-ready future to completion with a no-op waker and zero
/// heap allocation. The mock `exchange` never genuinely pends, so a poll-to-
/// `Ready` loop suffices.
#[allow(unsafe_code)]
fn block_on<F: Future>(mut f: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    const fn noop(_: *const ()) {}
    const fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VT)
    }
    static VT: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY:
    //   1. Waker: the vtable's clone/wake/drop are no-ops over a null data
    //      pointer, so `Waker::from_raw` is trivially valid and the pointer is
    //      never dereferenced.
    //   2. Pin: `f` is a local owned by this function and borrowed for the rest
    //      of its body; it is never moved before the pinned reference is
    //      dropped, satisfying `Pin::new_unchecked`'s contract.
    let w = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VT)) };
    let mut cx = Context::from_waker(&w);
    let mut f = unsafe { core::pin::Pin::new_unchecked(&mut f) };
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

const fn unit_scale() -> AxisScale {
    AxisScale {
        inc_per_unit: 1000.0,
        zero_offset: 0,
    }
}

/// Run `ticks` warm steady-state steps (empty commands, no fault), driving each
/// `async` step to completion with the allocation-free `block_on`.
fn run(nc: &mut NcCycle<N>, bus: &mut MockCyclicFieldbus, ticks: usize) {
    for _ in 0..ticks {
        let _ = block_on(nc.step(bus, &[], DT));
    }
}

#[test]
fn step_is_zero_alloc_in_steady_state() {
    let mut nc = NcCycle::<N>::new([unit_scale(); N]);
    nc.precompute();
    let mut bus = MockCyclicFieldbus::new(N);

    // Power all axes on and warm up to OperationEnabled OUTSIDE the measured
    // region — this absorbs the mock's one-time setup (Vec img/faulted/stale)
    // and the CiA-402 power-up state walk.
    for i in 0..N {
        nc.request_power(i, true);
    }
    for _ in 0..16 {
        let _ = block_on(nc.step(&mut bus, &[], DT));
        if (0..N).all(|i| nc.is_enabled(i)) {
            break;
        }
    }
    assert!((0..N).all(|i| nc.is_enabled(i)), "all axes should enable");

    // Further warm to absorb anything else one-shot (there shouldn't be any).
    run(&mut nc, &mut bus, TICKS_SMALL);

    let (a_small, ()) = count_allocs(|| run(&mut nc, &mut bus, TICKS_SMALL));
    let (a_big, ()) = count_allocs(|| run(&mut nc, &mut bus, TICKS_BIG));

    let diff = i64::try_from(a_big).unwrap() - i64::try_from(a_small).unwrap();
    let iters = i64::try_from(TICKS_BIG - TICKS_SMALL).unwrap();
    let per_iter = (diff + iters - 1) / iters; // round up

    assert_eq!(
        per_iter, 0,
        "per-step allocations: {per_iter} (small={a_small}, big={a_big})"
    );
}
