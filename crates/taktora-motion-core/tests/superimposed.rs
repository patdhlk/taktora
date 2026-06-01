//! Superimposed corrective motion (PLCopen `MC_MoveSuperimposed`): a
//! jerk-limited offset added on top of an axis's ongoing base motion via
//! `Axis::superimpose`, applied additively in `AxisGroup::tick`.

#![allow(clippy::doc_markdown)]

use proptest::prelude::*;
use taktora_motion_core::couple::Gear;
use taktora_motion_core::profile::SCurveState;
use taktora_motion_core::state::Limits;
use taktora_motion_core::{Axis, AxisGroup, AxisState, Motion, master};

const DT: f64 = 0.001;

const fn lim() -> Limits {
    Limits::kinematic(10.0, 50.0, 500.0)
}

#[test]
fn superimposed_on_idle_equals_the_offset_scurve() {
    // With an Idle base (held at 0), the axis output is exactly the corrective
    // S-curve `0 -> delta`.
    let mut a = Axis::new(Motion::Idle(0.0));
    a.superimpose(7.0, lim()).unwrap();
    let mut group = AxisGroup::new([a], [0]);
    let mut reference = SCurveState::plan(AxisState::ZERO, 7.0, lim()).unwrap();

    for _ in 0..5_000 {
        group.tick(DT);
        let r = reference.update(DT);
        let s = group.state(0);
        assert!((s.pos - r.pos).abs() < 1e-12, "pos {} != {}", s.pos, r.pos);
        assert!((s.vel - r.vel).abs() < 1e-12);
        assert!((s.acc - r.acc).abs() < 1e-12);
    }
    let s = group.state(0);
    assert!((s.pos - 7.0).abs() < 1e-9, "final {}", s.pos);
    assert!(s.vel.abs() < 1e-9);
}

#[test]
fn superimposed_adds_to_a_moving_base() {
    // Base: slave geared 1:1 to a moving master, so base slave_pos == master_pos.
    // The superimposed offset must show up as exactly (slave - master).
    let mut slave = Axis::geared(Gear::new(1.0), 0);
    slave.superimpose(10.0, lim()).unwrap();
    let mut group = AxisGroup::new([master::velocity(5.0, 1000.0), slave], [0, 1]);
    let mut overlay_ref = SCurveState::plan(AxisState::ZERO, 10.0, lim()).unwrap();

    for _ in 0..6_000 {
        group.tick(DT);
        let r = overlay_ref.update(DT);
        let diff = group.state(1).pos - group.state(0).pos;
        assert!((diff - r.pos).abs() < 1e-9, "offset {} != {}", diff, r.pos);
    }
    // Offset reached and holds at delta; slave tracks master + 10 thereafter.
    let diff = group.state(1).pos - group.state(0).pos;
    assert!((diff - 10.0).abs() < 1e-9, "held offset {diff}");
    assert!((group.state(1).vel - group.state(0).vel).abs() < 1e-9);
}

#[test]
fn no_overlay_leaves_the_base_untouched() {
    let slave = Axis::geared(Gear::new(1.0), 0);
    let mut group = AxisGroup::new([master::velocity(5.0, 1000.0), slave], [0, 1]);
    for _ in 0..1_000 {
        group.tick(DT);
        assert!((group.state(1).pos - group.state(0).pos).abs() < 1e-12);
    }
    assert!(!group.axis(1).superimposed_active());
}

#[test]
fn active_flag_and_clear() {
    let mut a = Axis::new(Motion::Idle(0.0));
    a.superimpose(3.0, lim()).unwrap();
    let mut group = AxisGroup::new([a], [0]);

    group.tick(DT);
    assert!(
        group.axis(0).superimposed_active(),
        "should be active mid-move"
    );

    // Run to completion: the corrective finishes but the offset persists.
    for _ in 0..5_000 {
        group.tick(DT);
    }
    assert!(!group.axis(0).superimposed_active(), "done -> not active");
    assert!(
        (group.state(0).pos - 3.0).abs() < 1e-9,
        "offset held at delta"
    );

    // Clearing a completed offset steps the command back by delta.
    group.axis_mut(0).clear_superimposed();
    group.tick(DT);
    assert!(
        group.state(0).pos.abs() < 1e-9,
        "offset removed -> back to base 0"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn overlay_reaches_delta_and_is_monotonic(
        delta in -30.0_f64..30.0,
        base in -10.0_f64..10.0,
    ) {
        let mut a = Axis::new(Motion::Idle(base));
        a.superimpose(delta, lim()).unwrap();
        let mut group = AxisGroup::new([a], [0]);

        let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
        let mut progress = 0.0_f64;
        for _ in 0..20_000 {
            group.tick(DT);
            // Offset travelled so far = output - base, measured along the move.
            let travelled = (group.state(0).pos - base) * dir;
            prop_assert!(travelled >= progress - 1e-9, "non-monotonic");
            progress = travelled.max(progress);
            if !group.axis(0).superimposed_active() {
                break;
            }
        }
        // Base + delta reached and at rest.
        let s = group.state(0);
        prop_assert!((s.pos - (base + delta)).abs() < 1e-6, "final {} != {}", s.pos, base + delta);
        prop_assert!(s.vel.abs() < 1e-6);
    }
}
