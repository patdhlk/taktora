//! Scaffold smoke tests: every architectural seam exercised once through the
//! public API. These are coarse sanity checks, not the trajectory-accuracy
//! suite (that's the differential-oracle work, deferred).

use taktora_motion_core::couple::Gear;
use taktora_motion_core::profile::TrapState;
use taktora_motion_core::state::Limits;
use taktora_motion_core::{Axis, AxisGroup, AxisState, AxisStatus, Motion, master};

const DT: f64 = 0.001;

#[test]
fn virtual_master_velocity_advances() {
    // A virtual master is just an Axis running a Velocity profile.
    let mut group = AxisGroup::new([master::velocity(10.0, 100.0)], [0]);
    for _ in 0..1000 {
        group.tick(DT);
    }
    let s = group.state(0);
    // After 1 s it has ramped to 10 u/s and is cruising.
    assert!((s.vel - 10.0).abs() < 1e-9, "vel = {}", s.vel);
    assert!(s.pos > 9.0, "pos = {}", s.pos);
    assert_eq!(group.axis(0).status(), AxisStatus::ContinuousMotion);
}

#[test]
fn gear_follows_master_same_cycle() {
    // Axis 0: master jogging; axis 1: geared 2:1 to it.
    let master = master::velocity(5.0, 1000.0);
    let slave = Axis::geared(Gear::new(2.0), 0);
    let mut group = AxisGroup::new([master, slave], [0, 1]);

    for _ in 0..500 {
        group.tick(DT);
    }
    let m = group.state(0);
    let s = group.state(1);
    // Same-cycle coherence: slave is exactly 2x the master this very tick.
    assert!(
        (s.pos - 2.0 * m.pos).abs() < 1e-12,
        "slave {} vs master {}",
        s.pos,
        m.pos
    );
    assert!((s.vel - 2.0 * m.vel).abs() < 1e-12);
    assert_eq!(group.axis(1).status(), AxisStatus::SynchronizedMotion);
}

#[test]
fn trapezoid_reaches_target_and_rests() {
    let limits = Limits::new(2.0, 10.0, 100.0, -100.0, 100.0).unwrap();
    let trap = TrapState::plan(AxisState::ZERO, 5.0, limits).unwrap();
    let mut group = AxisGroup::new([Axis::new(Motion::Trapezoid(trap))], [0]);

    // Run well past the move duration.
    for _ in 0..5000 {
        group.tick(DT);
    }
    let s = group.state(0);
    assert!((s.pos - 5.0).abs() < 1e-9, "pos = {}", s.pos);
    assert!(s.vel.abs() < 1e-9, "vel = {}", s.vel);
    assert_eq!(group.axis(0).status(), AxisStatus::Standstill);
}

#[test]
fn trapezoid_rejects_out_of_limits_target() {
    let limits = Limits::new(2.0, 10.0, 100.0, -1.0, 1.0).unwrap();
    let err = TrapState::plan(AxisState::ZERO, 5.0, limits);
    assert!(err.is_err());
}

#[test]
fn modulo_wraps_rotary_master() {
    // Endless rotary master with a 360-unit modulo, jogging fast.
    let master = master::velocity(720.0, 100_000.0).with_modulo(360.0);
    let mut group = AxisGroup::new([master], [0]);
    for _ in 0..1000 {
        group.tick(DT);
    }
    let s = group.state(0);
    assert!((0.0..360.0).contains(&s.pos), "pos = {} not wrapped", s.pos);
}
