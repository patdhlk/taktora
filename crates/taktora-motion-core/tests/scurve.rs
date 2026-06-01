//! S-curve (jerk-limited) profile verification: analytic spot-checks plus
//! property invariants. The key strengthening over the trapezoid is **C²**
//! continuity — acceleration is continuous and jerk is bounded by `j_max`.
//! Time-optimality (correct case selection) is cross-checked against the
//! Ruckig oracle in `taktora-motion-core-tests/tests/scurve_oracle.rs`.

#![allow(clippy::doc_markdown)]

use proptest::prelude::*;
use taktora_motion_core::AxisState;
use taktora_motion_core::profile::SCurveState;
use taktora_motion_core::state::Limits;

fn at(p0: f64, target: f64, lim: Limits, t: f64) -> AxisState {
    let mut s = SCurveState::plan(AxisState::at(p0), target, lim).unwrap();
    s.update(t)
}

#[test]
fn analytic_all_limits_reached() {
    // j=10, a=5, v=10, D=50 -> Tj=0.5, Ta=2.5, Tv=2.5, total=7.5.
    let lim = Limits::new(10.0, 5.0, 10.0, -100.0, 100.0).unwrap();

    // End of first jerk segment (t=0.5): acceleration has ramped to a_max.
    let s = at(0.0, 50.0, lim, 0.5);
    assert!((s.acc - 5.0).abs() < 1e-9, "acc {}", s.acc);
    assert!((s.vel - 1.25).abs() < 1e-9, "vel {}", s.vel); // 0.5*j*Tj^2

    // Mid-cruise (t=3.75): at v_max, zero acceleration, halfway in distance.
    let s = at(0.0, 50.0, lim, 3.75);
    assert!((s.vel - 10.0).abs() < 1e-9, "cruise vel {}", s.vel);
    assert!(s.acc.abs() < 1e-9, "cruise acc {}", s.acc);
    assert!((s.pos - 25.0).abs() < 1e-9, "cruise pos {}", s.pos);

    // Past the end: rest exactly at the target.
    let s = at(0.0, 50.0, lim, 8.0);
    assert!((s.pos - 50.0).abs() < 1e-9);
    assert!(s.vel.abs() < 1e-12);
    assert!(s.acc.abs() < 1e-12);
}

#[test]
fn acceleration_is_continuous_at_zero_crossing() {
    // Unlike a trapezoid (which steps acceleration), the S-curve passes through
    // zero acceleration smoothly at the start.
    let lim = Limits::new(10.0, 5.0, 10.0, -100.0, 100.0).unwrap();
    let s = at(0.0, 50.0, lim, 0.0);
    assert!(s.acc.abs() < 1e-12, "start acc must be 0, got {}", s.acc);
    assert!(s.vel.abs() < 1e-12, "start vel must be 0, got {}", s.vel);
}

#[test]
fn negative_direction_is_mirror() {
    let lim = Limits::new(10.0, 5.0, 10.0, -100.0, 100.0).unwrap();
    let fwd = at(0.0, 50.0, lim, 1.3);
    let rev = at(0.0, -50.0, lim, 1.3);
    assert!((fwd.pos + rev.pos).abs() < 1e-9);
    assert!((fwd.vel + rev.vel).abs() < 1e-9);
    assert!((fwd.acc + rev.acc).abs() < 1e-9);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn move_respects_limits_is_c2_and_reaches_target(
        p0 in -30.0_f64..30.0,
        delta in -30.0_f64..30.0,
        v_max in 5.0_f64..30.0,
        a_max in 80.0_f64..400.0,
        j_max in 500.0_f64..3000.0,
    ) {
        let target = p0 + delta;
        let lim = Limits::new(v_max, a_max, j_max, -200.0, 200.0).unwrap();
        let mut s = SCurveState::plan(AxisState::at(p0), target, lim).unwrap();

        let dt = 0.001;
        let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
        let v_eps = v_max * 1e-6 + 1e-9;
        let a_eps = a_max * 1e-6 + 1e-9;
        let j_eps = j_max * 1e-6 + 1e-6;

        let mut prev = AxisState::at(p0);
        let mut progress = 0.0_f64;
        let mut iters = 0;
        loop {
            let st = s.update(dt);

            // Kinematic envelope.
            prop_assert!(st.vel.abs() <= v_max + v_eps, "|vel|={} > v_max", st.vel.abs());
            prop_assert!(st.acc.abs() <= a_max + a_eps, "|acc|={} > a_max", st.acc.abs());

            // C0/C1/C2 continuity: position, velocity, AND acceleration are
            // Lipschitz with constants v_max / a_max / j_max respectively.
            prop_assert!((st.pos - prev.pos).abs() <= v_max * dt + v_eps);
            prop_assert!((st.vel - prev.vel).abs() <= a_max * dt + a_eps);
            prop_assert!((st.acc - prev.acc).abs() <= j_max * dt + j_eps,
                "jerk exceeded: |dacc|={} > j_max*dt", (st.acc - prev.acc).abs());

            // Monotonic toward the target (rest-to-rest never overshoots).
            let travelled = (st.pos - p0) * dir;
            prop_assert!(travelled >= progress - v_eps);
            progress = travelled.max(progress);

            prev = st;
            if s.done() {
                break;
            }
            iters += 1;
            prop_assert!(iters < 200_000, "did not terminate");
        }

        prop_assert!((prev.pos - target).abs() < 1e-6, "final pos {} != target {}", prev.pos, target);
        prop_assert!(prev.vel.abs() < 1e-6, "final vel {}", prev.vel);
        prop_assert!(prev.acc.abs() < 1e-6, "final acc {}", prev.acc);
    }
}
