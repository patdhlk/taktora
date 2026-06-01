//! Differential oracle for the S-curve: the alloc-free `SCurveState` must match
//! Ruckig's time-optimal jerk-limited trajectory (same total duration, same
//! sampled position) for rest-to-rest moves. This validates the *case
//! selection* (reaches a_max? reaches v_max?) that the analytic invariants in
//! `taktora-motion-core/tests/scurve.rs` cannot pin down on their own.

#![allow(clippy::doc_markdown)]

use proptest::prelude::*;
use rsruckig::prelude::*;
use taktora_motion_core::AxisState;
use taktora_motion_core::profile::SCurveState;
use taktora_motion_core::state::Limits;

/// Total duration + sampled positions of Ruckig's offline time-optimal profile.
fn ruckig_profile(p0: f64, target: f64, v: f64, a: f64, j: f64) -> (f64, Vec<(f64, f64)>) {
    let mut otg = Ruckig::<1, ThrowErrorHandler>::new(None, 0.001);
    let mut input = InputParameter::new(None);
    input.current_position[0] = p0;
    input.current_velocity[0] = 0.0;
    input.current_acceleration[0] = 0.0;
    input.target_position[0] = target;
    input.target_velocity[0] = 0.0;
    input.target_acceleration[0] = 0.0;
    input.max_velocity[0] = v;
    input.max_acceleration[0] = a;
    input.max_jerk[0] = j;

    let mut traj = Trajectory::<1>::new(None);
    otg.calculate(&input, &mut traj).expect("ruckig calculate");
    let duration = traj.duration;

    let mut samples = Vec::new();
    let mut pos = DataArrayOrVec::Stack([0.0]);
    for k in 1..10 {
        let t = duration * f64::from(k) / 10.0;
        traj.at_time(
            t,
            &mut Some(&mut pos),
            &mut None,
            &mut None,
            &mut None,
            &mut None,
        );
        samples.push((t, pos[0]));
    }
    (duration, samples)
}

/// Sample our S-curve position at absolute time `t` (fresh plan each call).
fn ours_at(p0: f64, target: f64, lim: Limits, t: f64) -> f64 {
    let mut s = SCurveState::plan(AxisState::at(p0), target, lim).unwrap();
    s.update(t).pos
}

fn compare(p0: f64, target: f64, v: f64, a: f64, j: f64) {
    let lim = Limits::new(v, a, j, -10_000.0, 10_000.0).unwrap();
    let (dur_ruckig, samples) = ruckig_profile(p0, target, v, a, j);

    let dur_ours = SCurveState::plan(AxisState::at(p0), target, lim)
        .unwrap()
        .duration();

    let d = (target - p0).abs().max(1.0);
    assert!(
        (dur_ours - dur_ruckig).abs() < 5e-3 + 1e-3 * dur_ruckig,
        "duration mismatch: ours={dur_ours} ruckig={dur_ruckig}"
    );

    for (t, p_ruckig) in samples {
        let p_ours = ours_at(p0, target, lim, t);
        assert!(
            (p_ours - p_ruckig).abs() < 1e-3 * d + 1e-4,
            "pos mismatch at t={t}: ours={p_ours} ruckig={p_ruckig} (D={d})"
        );
    }
}

#[test]
fn all_limits_reached() {
    // j=10,a=5,v=10,D=50 -> the textbook full 7-segment profile (total 7.5 s).
    compare(0.0, 50.0, 10.0, 5.0, 10.0);
}

#[test]
fn v_max_not_reached() {
    // Short move: cruise collapses, peak velocity below v_max.
    compare(0.0, 2.0, 50.0, 20.0, 100.0);
}

#[test]
fn neither_limit_reached() {
    // Very short move with low jerk: triangular accel, no const-accel, no cruise.
    compare(0.0, 0.05, 50.0, 50.0, 30.0);
}

#[test]
fn a_max_not_reached_but_v_max_is() {
    // High a_max, low j_max relative to v_max: v*j < a^2, so a_max isn't reached
    // even though v_max is.
    compare(0.0, 40.0, 8.0, 50.0, 20.0);
}

#[test]
fn negative_direction() {
    compare(10.0, -30.0, 12.0, 30.0, 60.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Sweep the parameter space so the *boundaries* between case-selection
    /// branches (where `v·j = a²` or where the cruise/const-accel segment
    /// collapses to zero) are exercised, not just the hand-picked interiors.
    #[test]
    fn matches_ruckig_over_parameter_sweep(
        p0 in -20.0_f64..20.0,
        delta in -50.0_f64..50.0,
        v in 3.0_f64..40.0,
        a in 10.0_f64..400.0,
        j in 20.0_f64..3000.0,
    ) {
        let target = p0 + delta;
        let lim = Limits::new(v, a, j, -10_000.0, 10_000.0).unwrap();
        let (dur_ruckig, samples) = ruckig_profile(p0, target, v, a, j);
        let dur_ours = SCurveState::plan(AxisState::at(p0), target, lim).unwrap().duration();
        let d = delta.abs().max(1.0);

        prop_assert!(
            (dur_ours - dur_ruckig).abs() < 5e-3 + 2e-3 * dur_ruckig,
            "duration: ours={dur_ours} ruckig={dur_ruckig}"
        );
        for (t, p_ruckig) in samples {
            let p_ours = ours_at(p0, target, lim, t);
            prop_assert!(
                (p_ours - p_ruckig).abs() < 2e-3 * d + 1e-4,
                "pos at t={t}: ours={p_ours} ruckig={p_ruckig} (D={d})"
            );
        }
    }
}
