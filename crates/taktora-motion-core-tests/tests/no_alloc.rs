//! REQ_0060 envelope for the motion core: ticking an `AxisGroup` performs
//! **zero** heap allocations in steady state. Mirrors the executor's
//! `CountingAllocator` harness (the TEST_0170 pattern) — a global counting
//! allocator with a tracking window, differential `big - small` measurement to
//! cancel any one-time setup, single-threaded so nothing else perturbs the
//! count.
//!
//! `taktora-motion-core` is `no_std` and allocation-free by construction (fixed
//! arrays, no `Box`/`Vec`); this test is the regression guard that keeps it so.

#![allow(clippy::doc_markdown)]

use taktora_bounded_alloc::CountingAllocator;
use taktora_motion_core::couple::{Cam, CamSegment, CamTable, FlyingSaw, Gear};
use taktora_motion_core::profile::{SCurveState, TrapState};
use taktora_motion_core::state::Limits;
use taktora_motion_core::{Axis, AxisGroup, AxisState, Motion, master};

/// A `&'static` single-segment linear cam table for the no-alloc group.
static CAM_SEGMENTS: [CamSegment; 1] = [CamSegment::linear(2.0, 0.0, 0.0)];

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();

const TICKS_SMALL: usize = 1_000;
const TICKS_BIG: usize = 10_000;

fn count_allocs<R>(f: impl FnOnce() -> R) -> (usize, R) {
    ALLOC.reset();
    ALLOC.set_tracking(true);
    let r = f();
    ALLOC.set_tracking(false);
    (ALLOC.alloc_count(), r)
}

/// Build a representative group exercising **every** generator on each tick: a
/// virtual master, a trapezoid, an S-curve (multi-segment walk), a geared rotary
/// slave carrying a superimposed corrective overlay, a flying-saw (quintic phase
/// machine), and a cam (binary-search + Horner). All read master axis 0 where
/// coupled.
fn build_group() -> AxisGroup<6> {
    let lim = Limits::new(10.0, 50.0, 100.0, -1000.0, 1000.0).unwrap();
    let trap = TrapState::plan(AxisState::ZERO, 250.0, lim).unwrap();
    let scurve = SCurveState::plan(AxisState::ZERO, 250.0, lim).unwrap();
    let saw = FlyingSaw::plan(
        0.0,
        2.0,
        1.0,
        AxisState::new(0.0, 20.0, 0.0),
        Limits::kinematic(200.0, 2000.0, 200_000.0),
    )
    .unwrap();
    let cam = Cam::new(CamTable::new(&CAM_SEGMENTS, 360.0));
    // Geared slave with a superimposed corrective offset — exercises the
    // additive-overlay branch of AxisGroup::tick.
    let mut geared = Axis::geared(Gear::new(2.0), 0).with_modulo(360.0);
    geared.superimpose(45.0, lim).unwrap();
    let axes = [
        master::velocity(20.0, 100.0),
        Axis::new(Motion::Trapezoid(trap)),
        Axis::new(Motion::SCurve(scurve)),
        geared,
        Axis::new(Motion::FlyingSaw(saw)).with_master(0),
        Axis::new(Motion::Cam(cam)).with_master(0),
    ];
    AxisGroup::new(axes, [0, 1, 2, 3, 4, 5])
}

fn run(group: &mut AxisGroup<6>, ticks: usize) {
    for _ in 0..ticks {
        group.tick(0.001);
    }
}

#[test]
fn axis_group_tick_is_zero_alloc_in_steady_state() {
    let mut group = build_group();

    // Warm: absorb anything one-shot (there shouldn't be any).
    run(&mut group, TICKS_SMALL);

    let (a_small, ()) = count_allocs(|| run(&mut group, TICKS_SMALL));
    let (a_big, ()) = count_allocs(|| run(&mut group, TICKS_BIG));

    let diff = i64::try_from(a_big).unwrap() - i64::try_from(a_small).unwrap();
    let iters = i64::try_from(TICKS_BIG - TICKS_SMALL).unwrap();
    let per_iter = (diff + iters - 1) / iters; // round up

    assert_eq!(
        per_iter, 0,
        "per-tick allocations: {per_iter} (small={a_small}, big={a_big})"
    );
}
