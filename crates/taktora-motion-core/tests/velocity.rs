//! Velocity (jog) profile verification: ramps to the commanded velocity at
//! `a_max`, respects the acceleration envelope, and integrates position.

#![allow(clippy::doc_markdown)]

use proptest::prelude::*;
use taktora_motion_core::AxisState;
use taktora_motion_core::profile::VelocityMove;

#[test]
fn analytic_ramp_from_rest() {
    // target 10 u/s at a_max 5 u/s^2 -> reaches in 2 s; at t=1 s, vel=5.
    let mut v = VelocityMove::new(AxisState::ZERO, 10.0, 5.0);
    let dt = 0.001;
    let mut s = AxisState::ZERO;
    for _ in 0..1000 {
        s = v.update(dt); // last sample is at t = 1.0 s
    }
    assert!((s.vel - 5.0).abs() < 1e-3, "vel {}", s.vel);
    assert!((s.acc - 5.0).abs() < 1e-9, "acc {} (still ramping)", s.acc);
}

#[test]
fn holds_after_reaching_target() {
    let mut v = VelocityMove::new(AxisState::ZERO, 4.0, 100.0);
    let dt = 0.001;
    for _ in 0..1000 {
        let _ = v.update(dt);
    }
    let s = v.update(dt);
    assert!((s.vel - 4.0).abs() < 1e-9, "vel {}", s.vel);
    assert!(s.acc.abs() < 1e-9, "acc {} (should be holding)", s.acc);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn ramp_is_monotonic_and_bounded(
        target_vel in -20.0_f64..20.0,
        a_max in 10.0_f64..500.0,
    ) {
        let mut v = VelocityMove::new(AxisState::ZERO, target_vel, a_max);
        let dt = 0.001;
        let a_eps = a_max * 1e-6 + 1e-9;

        let mut prev_vel = 0.0_f64;
        let toward = if target_vel >= 0.0 { 1.0 } else { -1.0 };
        let mut iters = 0;
        loop {
            let s = v.update(dt);

            // Acceleration envelope.
            prop_assert!(s.acc.abs() <= a_max + a_eps, "|acc|={} > a_max={}", s.acc.abs(), a_max);
            // C1 continuity: velocity step bounded by a_max*dt.
            prop_assert!((s.vel - prev_vel).abs() <= a_max * dt + a_eps);
            // Monotonic toward the commanded velocity, never past it.
            prop_assert!(s.vel * toward >= prev_vel * toward - a_eps);
            prop_assert!(s.vel * toward <= target_vel.abs() + a_eps);

            prev_vel = s.vel;
            if (s.vel - target_vel).abs() < 1e-9 {
                break;
            }
            iters += 1;
            prop_assert!(iters < 100_000, "ramp did not converge");
        }
        prop_assert!((prev_vel - target_vel).abs() < 1e-9);
    }
}
