//! Coupling + modulo verification through `AxisGroup`: electronic gearing is
//! same-cycle coherent, slaves read the *wrapped* master position, rotary axes
//! stay within their modulo window, and ticking is deterministic.

use proptest::prelude::*;
use taktora_motion_core::couple::Gear;
use taktora_motion_core::{Axis, AxisGroup, Motion, master};

#[test]
fn slave_reads_wrapped_master_position() {
    // Master is rotary (modulo 360) jogging fast enough to wrap; an unscaled
    // gear slave must follow the *wrapped* value, proving modulo is applied in
    // tick() before the slave reads it.
    let m = master::velocity(720.0, 1_000_000.0).with_modulo(360.0);
    let slave = Axis::geared(Gear::new(1.0), 0);
    let mut group = AxisGroup::new([m, slave], [0, 1]);

    for _ in 0..1000 {
        group.tick(0.001);
    }
    let master_pos = group.state(0).pos;
    let slave_pos = group.state(1).pos;
    assert!(
        (0.0..360.0).contains(&master_pos),
        "master not wrapped: {master_pos}"
    );
    assert!(
        (slave_pos - master_pos).abs() < 1e-9,
        "slave {slave_pos} != wrapped master {master_pos}"
    );
}

#[test]
fn uncoupled_gear_holds_at_offset() {
    // A gear with no master axis available holds at its offset.
    let lone = Axis::new(Motion::Gear(Gear::with_offset(2.0, 7.5)));
    let mut group = AxisGroup::new([lone], [0]);
    group.tick(0.001);
    assert!((group.state(0).pos - 7.5).abs() < 1e-12);
    assert!(group.state(0).vel.abs() < 1e-12);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Same-cycle gearing: every tick, slave == ratio * master (+ offset),
    /// exactly, with no one-cycle lag.
    #[test]
    fn gear_tracks_master_each_cycle(
        ratio in -5.0_f64..5.0,
        offset in -100.0_f64..100.0,
        target_vel in 1.0_f64..30.0,
    ) {
        let m = master::velocity(target_vel, 200.0);
        let slave = Axis::geared(Gear::with_offset(ratio, offset), 0);
        let mut group = AxisGroup::new([m, slave], [0, 1]);

        for _ in 0..500 {
            group.tick(0.001);
            let ms = group.state(0);
            let ss = group.state(1);
            prop_assert!((ss.pos - (ratio * ms.pos + offset)).abs() < 1e-9);
            prop_assert!((ss.vel - ratio * ms.vel).abs() < 1e-9);
            prop_assert!((ss.acc - ratio * ms.acc).abs() < 1e-9);
        }
    }

    /// A rotary axis's published position is always inside `[0, modulo)`.
    #[test]
    fn rotary_position_stays_in_window(
        period in 1.0_f64..360.0,
        vel in -500.0_f64..500.0,
    ) {
        let m = master::velocity(vel, 1_000_000.0).with_modulo(period);
        let mut group = AxisGroup::new([m], [0]);
        for _ in 0..2000 {
            group.tick(0.001);
            let p = group.state(0).pos;
            prop_assert!((0.0..period).contains(&p), "pos {} outside [0,{})", p, period);
        }
    }

    /// Ticking is deterministic: identical inputs produce identical states.
    #[test]
    fn tick_is_deterministic(
        target_vel in -20.0_f64..20.0,
        ratio in -3.0_f64..3.0,
    ) {
        let build = || {
            let m = master::velocity(target_vel, 150.0);
            let slave = Axis::geared(Gear::new(ratio), 0);
            AxisGroup::new([m, slave], [0, 1])
        };
        let mut a = build();
        let mut b = build();
        for _ in 0..300 {
            a.tick(0.001);
            b.tick(0.001);
        }
        prop_assert_eq!(a.state(0).pos.to_bits(), b.state(0).pos.to_bits());
        prop_assert_eq!(a.state(1).pos.to_bits(), b.state(1).pos.to_bits());
    }
}
