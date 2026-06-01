//! Trapezoid profile verification: analytic spot-checks plus property
//! invariants (reaches target, respects v_max/a_max, C0/C1 continuity across
//! segment seams, monotonicity, symmetry). The jerk-limited oracle (Ruckig)
//! belongs to the deferred S-curve slice — a trapezoid has infinite jerk, which
//! Ruckig cannot represent, so the trapezoid is verified analytically.

#![allow(clippy::doc_markdown)]

use proptest::prelude::*;
use taktora_motion_core::AxisState;
use taktora_motion_core::profile::TrapState;
use taktora_motion_core::state::Limits;

/// Evaluate the profile at absolute time `t` (a fresh plan, advanced once).
fn at(p0: f64, target: f64, limits: Limits, t: f64) -> AxisState {
    let mut trap = TrapState::plan(AxisState::at(p0), target, limits).unwrap();
    trap.update(t)
}

#[test]
fn analytic_trapezoidal_case() {
    // D=10, v_max=2, a_max=10 -> t_acc=0.2, d_acc=0.2, t_flat=4.8, T=5.2.
    let lim = Limits::new(2.0, 10.0, 100.0, -100.0, 100.0).unwrap();

    // Mid-accel (t=0.1): pos=0.05, vel=1, acc=+10.
    let s = at(0.0, 10.0, lim, 0.1);
    assert!((s.pos - 0.05).abs() < 1e-12, "pos {}", s.pos);
    assert!((s.vel - 1.0).abs() < 1e-12, "vel {}", s.vel);
    assert!((s.acc - 10.0).abs() < 1e-12, "acc {}", s.acc);

    // Seam accel->cruise (t=0.2): pos=0.2, vel=2 (v_peak), acc=0.
    let s = at(0.0, 10.0, lim, 0.2);
    assert!((s.pos - 0.2).abs() < 1e-12);
    assert!((s.vel - 2.0).abs() < 1e-12);

    // Cruise (t=3.0): pos = 0.2 + 2*(3.0-0.2) = 5.8, vel=2, acc=0.
    let s = at(0.0, 10.0, lim, 3.0);
    assert!((s.pos - 5.8).abs() < 1e-12, "pos {}", s.pos);
    assert!((s.vel - 2.0).abs() < 1e-12);
    assert!(s.acc.abs() < 1e-12);

    // Past the end (t=6.0 > 5.2): rest at target.
    let s = at(0.0, 10.0, lim, 6.0);
    assert!((s.pos - 10.0).abs() < 1e-12);
    assert!(s.vel.abs() < 1e-12);
}

#[test]
fn analytic_triangular_case() {
    // Short move that never reaches v_max: D=1, v_max=10, a_max=4.
    // v_peak=sqrt(4*1)=2 (< 10), so triangular. t_acc=0.5, T=1.0, apex at t=0.5.
    let lim = Limits::new(10.0, 4.0, 100.0, -100.0, 100.0).unwrap();
    let apex = at(0.0, 1.0, lim, 0.5);
    assert!((apex.vel - 2.0).abs() < 1e-12, "peak vel {}", apex.vel);
    assert!((apex.pos - 0.5).abs() < 1e-12, "apex pos {}", apex.pos);
    // Peak velocity stayed well under v_max.
    assert!(apex.vel < 10.0);
}

#[test]
fn negative_direction_is_mirror() {
    let lim = Limits::new(2.0, 10.0, 100.0, -100.0, 100.0).unwrap();
    let fwd = at(0.0, 10.0, lim, 0.1);
    let rev = at(0.0, -10.0, lim, 0.1);
    assert!((fwd.pos + rev.pos).abs() < 1e-12);
    assert!((fwd.vel + rev.vel).abs() < 1e-12);
    assert!((fwd.acc + rev.acc).abs() < 1e-12);
}

#[test]
fn zero_distance_is_immediately_at_rest() {
    let lim = Limits::new(2.0, 10.0, 100.0, -100.0, 100.0).unwrap();
    let s = at(5.0, 5.0, lim, 0.001);
    assert!((s.pos - 5.0).abs() < 1e-12);
    assert!(s.vel.abs() < 1e-12);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Simulate a full move and assert the core invariants hold at every sample.
    #[test]
    fn move_respects_limits_and_reaches_target(
        p0 in -50.0_f64..50.0,
        delta in -50.0_f64..50.0,
        v_max in 5.0_f64..50.0,
        a_max in 50.0_f64..500.0,
    ) {
        let target = p0 + delta;
        let lim = Limits::new(v_max, a_max, 100.0, -200.0, 200.0).unwrap();
        let mut trap = TrapState::plan(AxisState::at(p0), target, lim).unwrap();

        let dt = 0.001;
        let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
        let v_eps = v_max * 1e-6 + 1e-9;
        let a_eps = a_max * 1e-6 + 1e-9;

        let mut prev = AxisState::at(p0);
        let mut progress = 0.0_f64; // signed distance travelled so far
        let mut iters = 0;
        loop {
            let s = trap.update(dt);

            // Envelope: never exceed configured velocity / acceleration.
            prop_assert!(s.vel.abs() <= v_max + v_eps, "|vel|={} > v_max={}", s.vel.abs(), v_max);
            prop_assert!(s.acc.abs() <= a_max + a_eps, "|acc|={} > a_max={}", s.acc.abs(), a_max);

            // C0 / C1 continuity across the cycle (incl. the terminal clamp,
            // whose residual is bounded because the profile ends at v=0).
            prop_assert!((s.pos - prev.pos).abs() <= v_max * dt + v_eps,
                "pos jump {} > v_max*dt", (s.pos - prev.pos).abs());
            prop_assert!((s.vel - prev.vel).abs() <= a_max * dt + a_eps,
                "vel jump {} > a_max*dt", (s.vel - prev.vel).abs());

            // Monotonic toward the target — a trapezoid never overshoots.
            let travelled = (s.pos - p0) * dir;
            prop_assert!(travelled >= progress - v_eps, "non-monotonic: {} < {}", travelled, progress);
            progress = travelled.max(progress);

            prev = s;
            if trap.done() {
                break;
            }
            iters += 1;
            prop_assert!(iters < 200_000, "move did not terminate");
        }

        // Reached the target and came to rest.
        prop_assert!((prev.pos - target).abs() < 1e-6, "final pos {} != target {}", prev.pos, target);
        prop_assert!(prev.vel.abs() < 1e-6, "final vel {}", prev.vel);
    }

    /// Acceleration and deceleration ramps are symmetric (t_acc == t_dec), so
    /// the peak velocity is reached at the temporal midpoint of a symmetric
    /// (flat-less, triangular) move.
    #[test]
    fn triangular_apex_is_centered(
        d in 0.5_f64..20.0,
        a_max in 10.0_f64..200.0,
    ) {
        // Force triangular: pick v_max above the triangular peak sqrt(a*d).
        let v_max = (a_max * d).sqrt() * 2.0;
        let lim = Limits::new(v_max, a_max, 100.0, -100.0, 100.0).unwrap();
        let peak = (a_max * d).sqrt();
        let t_acc = peak / a_max;

        let apex = at(0.0, d, lim, t_acc);
        prop_assert!((apex.vel - peak).abs() < 1e-6, "apex vel {} != peak {}", apex.vel, peak);
        prop_assert!((apex.pos - d / 2.0).abs() < 1e-6, "apex pos {} != d/2 {}", apex.pos, d / 2.0);
    }
}
