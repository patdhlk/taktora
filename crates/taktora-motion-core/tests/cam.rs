//! Electronic-cam verification: analytic spot-checks of the quintic profile and
//! the chain-rule derivatives, seam C¹/C² continuity, periodic wrap, and a
//! property sweep over a continuous periodic table. Test binaries are `std`, so
//! these use `f64::abs()` / `f64::powi()` directly.

use proptest::prelude::*;
use taktora_motion_core::AxisState;
use taktora_motion_core::couple::{Cam, CamSegment, CamTable};

/// Evaluate a quintic `[c0..c5]` at `u` and its first two derivatives, by hand,
/// for cross-checking the generator.
fn quintic(coeffs: [f64; 6], u: f64) -> (f64, f64, f64) {
    let [c0, c1, c2, c3, c4, c5] = coeffs;
    let p = c0 + c1 * u + c2 * u.powi(2) + c3 * u.powi(3) + c4 * u.powi(4) + c5 * u.powi(5);
    let dp = c1 + 2.0 * c2 * u + 3.0 * c3 * u.powi(2) + 4.0 * c4 * u.powi(3) + 5.0 * c5 * u.powi(4);
    let ddp = 2.0 * c2 + 6.0 * c3 * u + 12.0 * c4 * u.powi(2) + 20.0 * c5 * u.powi(3);
    (p, dp, ddp)
}

/// Build the standard quintic interpolation coefficients (in local coordinate
/// `u ∈ [0, len]`) matching `(p, p', p'')` at both ends. Mirrors the flying-saw
/// derivation; used to assemble C²-continuous test tables.
fn quintic_coeffs(p0: f64, v0: f64, a0: f64, p1: f64, v1: f64, a1: f64, len: f64) -> [f64; 6] {
    let h = p1 - p0;
    let t = len;
    let (t2, t3, t4, t5) = (t * t, t.powi(3), t.powi(4), t.powi(5));
    let c0 = p0;
    let c1 = v0;
    let c2 = 0.5 * a0;
    let c3 = (20.0 * h - (8.0 * v1 + 12.0 * v0) * t - (3.0 * a0 - a1) * t2) / (2.0 * t3);
    let c4 = (-30.0 * h + (14.0 * v1 + 16.0 * v0) * t + (3.0 * a0 - 2.0 * a1) * t2) / (2.0 * t4);
    let c5 = (12.0 * h - (6.0 * v1 + 6.0 * v0) * t - (a0 - a1) * t2) / (2.0 * t5);
    [c0, c1, c2, c3, c4, c5]
}

#[test]
fn linear_cam_reproduces_gearing() {
    // A 1-segment cam with slope k over period P behaves exactly like a gear of
    // ratio k, for master positions inside [0, P).
    const K: f64 = 2.5;
    static SEGS: [CamSegment; 1] = [CamSegment::linear(K, 0.0, 0.0)];
    let cam = Cam::new(CamTable::new(&SEGS, 100.0));

    for &(pos, vel, acc) in &[(0.0, 3.0, 0.5), (10.0, -4.0, 2.0), (37.5, 1.0, -1.0)] {
        let out = cam.update(0.0, Some(AxisState::new(pos, vel, acc)));
        assert!(
            (out.pos - K * pos).abs() < 1e-12,
            "pos {} != {K}*{pos}",
            out.pos
        );
        assert!(
            (out.vel - K * vel).abs() < 1e-12,
            "vel {} != {K}*{vel}",
            out.vel
        );
        assert!(
            (out.acc - K * acc).abs() < 1e-12,
            "acc {} != {K}*{acc}",
            out.acc
        );
    }
}

#[test]
fn analytic_quintic_segment() {
    // A known quintic over [0, 10): P(u) = 1 + 2u + 0.5u² + 0.1u³ - 0.01u⁴ + 0.002u⁵.
    const C: [f64; 6] = [1.0, 2.0, 0.5, 0.1, -0.01, 0.002];
    static SEGS: [CamSegment; 1] = [CamSegment::new(C, 0.0)];
    let cam = Cam::new(CamTable::new(&SEGS, 10.0));

    for &s in &[0.0, 2.5, 5.0, 9.9] {
        // Drive with unit master velocity, zero accel: slave_vel == P'(s) and
        // slave_acc == P''(s) directly.
        let out = cam.update(0.0, Some(AxisState::new(s, 1.0, 0.0)));
        let (p, dp, ddp) = quintic(C, s);
        assert!((out.pos - p).abs() < 1e-9, "pos@{s}: {} != {p}", out.pos);
        assert!((out.vel - dp).abs() < 1e-9, "P'@{s}: {} != {dp}", out.vel);
        assert!(
            (out.acc - ddp).abs() < 1e-9,
            "P''@{s}: {} != {ddp}",
            out.acc
        );
    }
}

#[test]
fn chain_rule_velocity_and_acceleration() {
    const C: [f64; 6] = [0.0, 1.0, -0.3, 0.2, 0.05, -0.004];
    static SEGS: [CamSegment; 1] = [CamSegment::new(C, 0.0)];
    let cam = Cam::new(CamTable::new(&SEGS, 20.0));

    let s = 6.0;
    let mv = 2.0; // master velocity
    let ma = 0.7; // master acceleration
    let (p, dp, ddp) = quintic(C, s);

    let out = cam.update(0.0, Some(AxisState::new(s, mv, ma)));
    let want_vel = dp * mv;
    let want_acc = ddp * mv * mv + dp * ma;
    assert!((out.pos - p).abs() < 1e-9);
    assert!(
        (out.vel - want_vel).abs() < 1e-9,
        "vel {} != {want_vel}",
        out.vel
    );
    assert!(
        (out.acc - want_acc).abs() < 1e-9,
        "acc {} != {want_acc}",
        out.acc
    );

    // Cross-check vel against a finite difference of pos over a tiny master step.
    let eps = 1e-6;
    let p_minus = cam.update(0.0, Some(AxisState::new(s - eps, 0.0, 0.0))).pos;
    let p_plus = cam.update(0.0, Some(AxisState::new(s + eps, 0.0, 0.0))).pos;
    let slope_fd = (p_plus - p_minus) / (2.0 * eps);
    assert!(
        (slope_fd - dp).abs() < 1e-4,
        "fd slope {slope_fd} != P' {dp}"
    );
    assert!((out.vel - slope_fd * mv).abs() < 1e-3, "vel vs fd*mv");
}

#[test]
fn seam_is_c2_continuous() {
    // Two quintic pieces over [0,10) and [10,20), matched in P, P', P'' at the
    // knot u=10 (master_start of the second segment). Period 20.
    // Piece A: (0,0,0) -> (5, 0.8, 0.0) over len 10.
    // Piece B: (5, 0.8, 0.0) -> (3, 0.0, 0.0) over len 10. Seam state matches.
    let a = quintic_coeffs(0.0, 0.0, 0.0, 5.0, 0.8, 0.0, 10.0);
    let b = quintic_coeffs(5.0, 0.8, 0.0, 3.0, 0.0, 0.0, 10.0);
    // SAFETY of correctness: coeffs computed above; embed via a leaked static is
    // not needed — use a Box leaked into 'static for the &'static slice.
    let segs: &'static [CamSegment] = Box::leak(Box::new([
        CamSegment::new(a, 0.0),
        CamSegment::new(b, 10.0),
    ]));
    let cam = Cam::new(CamTable::new(segs, 20.0));

    // Approach the seam from both sides with unit master velocity.
    let eps = 1e-7;
    let lo = cam.update(0.0, Some(AxisState::new(10.0 - eps, 1.0, 0.0)));
    let hi = cam.update(0.0, Some(AxisState::new(10.0 + eps, 1.0, 0.0)));
    assert!(
        (lo.pos - hi.pos).abs() < 1e-5,
        "pos jump {} vs {}",
        lo.pos,
        hi.pos
    );
    assert!(
        (lo.vel - hi.vel).abs() < 1e-4,
        "vel jump {} vs {}",
        lo.vel,
        hi.vel
    );
    assert!(
        (lo.acc - hi.acc).abs() < 1e-3,
        "acc jump {} vs {}",
        lo.acc,
        hi.acc
    );

    // And the seam value equals the matched knot position.
    assert!((lo.pos - 5.0).abs() < 1e-4, "seam pos {} != 5.0", lo.pos);
}

#[test]
fn periodic_wrap_repeats_profile() {
    const C: [f64; 6] = [1.0, 0.5, 0.2, -0.01, 0.0, 0.0];
    static SEGS: [CamSegment; 1] = [CamSegment::new(C, 0.0)];
    let period = 12.0;
    let cam = Cam::new(CamTable::new(&SEGS, period));

    for &base in &[3.0, 7.5, 11.0] {
        let here = cam.update(0.0, Some(AxisState::new(base, 1.0, 0.0)));
        let wrapped = cam.update(0.0, Some(AxisState::new(base + 3.0 * period, 1.0, 0.0)));
        assert!((here.pos - wrapped.pos).abs() < 1e-9, "pos@{base}");
        assert!((here.vel - wrapped.vel).abs() < 1e-9, "vel@{base}");
        assert!((here.acc - wrapped.acc).abs() < 1e-9, "acc@{base}");
    }
}

#[test]
fn none_master_holds_at_profile_origin() {
    const C: [f64; 6] = [4.0, 2.0, 0.0, 0.0, 0.0, 0.0];
    static SEGS: [CamSegment; 1] = [CamSegment::new(C, 0.0)];
    let cam = Cam::new(CamTable::new(&SEGS, 10.0));
    let out = cam.update(0.0, None);
    assert!(
        (out.pos - 4.0).abs() < 1e-12,
        "held pos {} != P(0)=4",
        out.pos
    );
    assert!(out.vel.abs() < 1e-12 && out.acc.abs() < 1e-12);
}

#[test]
fn degenerate_table_holds_at_zero() {
    static EMPTY: [CamSegment; 0] = [];
    static SEGS: [CamSegment; 1] = [CamSegment::linear(1.0, 0.0, 0.0)];

    let cam = Cam::new(CamTable::new(&EMPTY, 10.0));
    let out = cam.update(0.0, Some(AxisState::new(5.0, 2.0, 1.0)));
    assert_eq!(out, AxisState::ZERO);

    let bad_period = Cam::new(CamTable::new(&SEGS, 0.0));
    assert_eq!(
        bad_period.update(0.0, Some(AxisState::new(5.0, 2.0, 1.0))),
        AxisState::ZERO
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Random master state over a continuous periodic two-segment table:
    /// bounded, no panic, and slave_vel ≈ (local slope)·master_vel where the
    /// local slope is checked by a finite difference of slave pos.
    #[test]
    fn random_master_is_bounded_and_chain_rule_holds(
        pos in -1000.0_f64..1000.0,
        vel in -50.0_f64..50.0,
        acc in -50.0_f64..50.0,
    ) {
        // A fixed continuous, C²-matched periodic table (period 20).
        let a = quintic_coeffs(0.0, 0.0, 0.0, 5.0, 0.8, 0.0, 10.0);
        let b = quintic_coeffs(5.0, 0.8, 0.0, 0.0, 0.0, 0.0, 10.0);
        let segs: &'static [CamSegment] =
            Box::leak(Box::new([CamSegment::new(a, 0.0), CamSegment::new(b, 10.0)]));
        let cam = Cam::new(CamTable::new(segs, 20.0));

        let out = cam.update(0.0, Some(AxisState::new(pos, vel, acc)));
        prop_assert!(out.pos.is_finite() && out.vel.is_finite() && out.acc.is_finite());

        // Finite-difference the local slope from position (master vel/acc = 0
        // at the probe points so pos depends only on master position).
        let eps = 1e-6;
        let p_minus = cam.update(0.0, Some(AxisState::new(pos - eps, 0.0, 0.0))).pos;
        let p_plus = cam.update(0.0, Some(AxisState::new(pos + eps, 0.0, 0.0))).pos;
        let slope = (p_plus - p_minus) / (2.0 * eps);

        // Away from the seam the central difference is accurate; allow slack
        // scaled by |vel| for cases probing across the wrap/seam.
        let tol = 1e-2 + 1e-3 * vel.abs();
        prop_assert!((out.vel - slope * vel).abs() < tol,
            "vel {} != slope {} * vel {}", out.vel, slope, vel);
    }
}
