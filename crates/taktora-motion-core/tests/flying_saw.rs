//! Integration tests for the flying-saw catch-up coupling.
//!
//! Test binaries are `std`, so they may use `f64::abs()` etc. directly.

#![allow(clippy::doc_markdown)]

use proptest::prelude::*;
use taktora_motion_core::couple::{FlyingSaw, Phase};
use taktora_motion_core::{AxisState, Limits, MotionError};

const DT: f64 = 1e-3;

/// A generously-feasible engagement: slow line, roomy limits.
const fn feasible() -> (f64, f64, f64, AxisState, Limits) {
    let home = 0.0;
    let t_on = 1.0;
    let t_sync = 0.5;
    // Master moving at 1.0 unit/s, currently at 2.0.
    let master = AxisState::new(2.0, 1.0, 0.0);
    let limits = Limits::kinematic(10.0, 50.0, 2000.0);
    (home, t_on, t_sync, master, limits)
}

/// Drive a planned saw to completion, returning the recorded
/// `(t, AxisState, Phase)` trace (one entry per cycle).
fn run_to_home(mut saw: FlyingSaw, master_v: f64, master_p0: f64) -> Vec<(f64, AxisState, Phase)> {
    let mut trace = Vec::new();
    let mut t = 0.0;
    let mut master_pos = master_p0;
    // A bound far exceeding any cycle length keeps the test finite even
    // if a regression breaks the Waiting transition.
    for _ in 0..1_000_000 {
        // Provide a live master consistent with the const-v model.
        master_pos += master_v * DT;
        let m = AxisState::new(master_pos, master_v, 0.0);
        let s = saw.update(DT, Some(m));
        t += DT;
        trace.push((t, s, saw.phase()));
        if saw.done() {
            break;
        }
    }
    trace
}

#[test]
fn sync_on_reaches_master_state_without_step() {
    let (home, t_on, t_sync, master, limits) = feasible();
    let mut saw = FlyingSaw::plan(home, t_on, t_sync, master, limits).unwrap();

    // Drive only through the SyncOn phase, tracking the live master.
    let mut master_pos = master.pos;
    let mut last = AxisState::at(home);
    for _ in 0..2_000 {
        master_pos += master.vel * DT;
        let m = AxisState::new(master_pos, master.vel, 0.0);
        last = saw.update(DT, Some(m));
        if saw.phase() != Phase::SyncOn {
            break;
        }
    }
    // At the SyncOn->Synchronous seam the saw should have caught the line:
    // its velocity equals v_m and its position equals where the master is.
    assert!(
        (last.vel - master.vel).abs() < 1e-3,
        "saw vel {} should match v_m {}",
        last.vel,
        master.vel
    );
    // sync_pos = master.pos + v_m * t_on (const-v prediction). The live
    // master tracked identically, so the saw lands on it.
    let sync_pos = master.pos + master.vel * t_on;
    assert!(
        (last.pos - sync_pos).abs() < 1e-2,
        "saw pos {last:?} should reach sync_pos {sync_pos}",
    );
    // Acceleration matched to zero at the seam (C²).
    assert!(last.acc.abs() < 1e-1, "accel at seam {} ~ 0", last.acc);
}

#[test]
fn c2_continuity_across_all_seams() {
    let (home, t_on, t_sync, master, limits) = feasible();
    let saw = FlyingSaw::plan(home, t_on, t_sync, master, limits).unwrap();
    let trace = run_to_home(saw, master.vel, master.pos);

    assert!(!trace.is_empty());
    // Tolerances: Lipschitz bounds from the limits, plus float slack.
    let v_step = limits.a_max * DT + 1e-6;
    let a_step = limits.j_max * DT + 1e-3;

    let mut prev: Option<AxisState> = None;
    for (t, s, _phase) in &trace {
        if let Some(p) = prev {
            let dpos = (s.pos - p.pos).abs();
            let dvel = (s.vel - p.vel).abs();
            let dacc = (s.acc - p.acc).abs();
            // Position moves at most v_max*dt per cycle.
            assert!(
                dpos <= limits.v_max * DT + 1e-6,
                "t={t}: position jump {dpos}"
            );
            // Velocity is Lipschitz with constant a_max.
            assert!(dvel <= v_step, "t={t}: velocity step {dvel} > {v_step}");
            // Acceleration is Lipschitz with constant j_max — this is the
            // C² seam guarantee across home->SyncOn->Sync->Return->home.
            assert!(dacc <= a_step, "t={t}: accel step {dacc} > {a_step}");
        }
        prev = Some(*s);
    }
}

#[test]
fn quintic_bounded_and_monotonic_through_sync_on() {
    let (home, t_on, t_sync, master, limits) = feasible();
    let mut saw = FlyingSaw::plan(home, t_on, t_sync, master, limits).unwrap();

    let mut master_pos = master.pos;
    let mut last_pos = home;
    for _ in 0..2_000 {
        master_pos += master.vel * DT;
        let m = AxisState::new(master_pos, master.vel, 0.0);
        let s = saw.update(DT, Some(m));
        if saw.phase() == Phase::SyncOn {
            // Velocity stays within [0, v_max] for a forward catch-up.
            assert!(
                s.vel >= -1e-9 && s.vel <= limits.v_max + 1e-9,
                "vel {} out of [0, v_max]",
                s.vel
            );
            assert!(s.acc.abs() <= limits.a_max + 1e-6, "acc {} > a_max", s.acc);
            // Position advances monotonically toward the sync point.
            assert!(s.pos >= last_pos - 1e-9, "pos went backwards");
            last_pos = s.pos;
        } else {
            break;
        }
    }
}

#[test]
fn infeasible_engagement_detected() {
    let home = 0.0;
    let t_on = 0.05; // very short catch-up window
    let t_sync = 0.5;
    // Fast master vs tight limits => the quintic must exceed a_max/j_max.
    let master = AxisState::new(0.0, 8.0, 0.0);
    let limits = Limits::kinematic(10.0, 5.0, 50.0);

    let res = FlyingSaw::plan(home, t_on, t_sync, master, limits);
    assert_eq!(res.err(), Some(MotionError::InfeasibleEngagement));
}

#[test]
fn non_positive_inputs_rejected() {
    let master = AxisState::new(0.0, 1.0, 0.0);
    let limits = Limits::kinematic(10.0, 50.0, 2000.0);
    assert_eq!(
        FlyingSaw::plan(0.0, 0.0, 0.5, master, limits).err(),
        Some(MotionError::NonPositiveDuration)
    );
    assert_eq!(
        FlyingSaw::plan(0.0, 1.0, -0.1, master, limits).err(),
        Some(MotionError::NonPositiveDuration)
    );
    let bad = Limits::kinematic(0.0, 50.0, 2000.0);
    assert_eq!(
        FlyingSaw::plan(0.0, 1.0, 0.5, master, bad).err(),
        Some(MotionError::NonPositiveLimit)
    );
}

#[test]
fn full_cycle_returns_home_at_rest() {
    let (home, t_on, t_sync, master, limits) = feasible();
    let saw = FlyingSaw::plan(home, t_on, t_sync, master, limits).unwrap();
    let trace = run_to_home(saw, master.vel, master.pos);

    let (_, last, phase) = *trace.last().unwrap();
    assert_eq!(phase, Phase::Waiting, "should end in Waiting");
    assert!((last.pos - home).abs() < 1e-6, "pos {} != home", last.pos);
    assert!(last.vel.abs() < 1e-6, "vel {} != 0", last.vel);
    assert!(last.acc.abs() < 1e-6, "acc {} != 0", last.acc);

    // The cycle visited every phase in order.
    let phases: Vec<Phase> = trace.iter().map(|(_, _, p)| *p).collect();
    assert!(phases.contains(&Phase::SyncOn));
    assert!(phases.contains(&Phase::Synchronous));
    assert!(phases.contains(&Phase::Return));
    assert!(phases.contains(&Phase::Waiting));
}

#[test]
fn rearm_runs_a_second_cycle() {
    let (home, t_on, t_sync, master, limits) = feasible();
    let mut saw = FlyingSaw::plan(home, t_on, t_sync, master, limits).unwrap();
    // Run first cycle to completion.
    let mut master_pos = master.pos;
    for _ in 0..1_000_000 {
        master_pos += master.vel * DT;
        saw.update(DT, Some(AxisState::new(master_pos, master.vel, 0.0)));
        if saw.done() {
            break;
        }
    }
    assert!(saw.done());
    // Re-arm against a fresh piece of material that has just entered the
    // catch-up zone near home (the previous piece has long since run off
    // downstream; the saw catches the *next* one, which is near home).
    saw.rearm(AxisState::new(master.pos, master.vel, 0.0))
        .unwrap();
    assert!(!saw.done());
    assert_eq!(saw.phase(), Phase::SyncOn);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn feasible_sweep_is_c2_reaches_sync_and_returns_home(
        v_m in 0.2f64..3.0,
        t_on in 0.6f64..2.0,
        t_sync in 0.1f64..1.0,
        master_p0 in -5.0f64..5.0,
    ) {
        // Roomy limits so the engagement is feasible across the sweep.
        let limits = Limits::kinematic(50.0, 500.0, 50_000.0);
        let home = 0.0;
        let master = AxisState::new(master_p0, v_m, 0.0);

        let saw = FlyingSaw::plan(home, t_on, t_sync, master, limits)
            .expect("feasible by construction");
        let trace = run_to_home(saw, v_m, master_p0);
        prop_assert!(!trace.is_empty());

        // C² continuity across all seams.
        let v_step = limits.a_max * DT + 1e-6;
        let a_step = limits.j_max * DT + 1e-2;
        let mut prev: Option<AxisState> = None;
        let mut saw_sync = false;
        for (_, s, phase) in &trace {
            if *phase == Phase::Synchronous {
                saw_sync = true;
                // During sync the saw tracks the line at v_m.
                prop_assert!((s.vel - v_m).abs() < 1e-6);
            }
            if let Some(p) = prev {
                prop_assert!((s.vel - p.vel).abs() <= v_step);
                prop_assert!((s.acc - p.acc).abs() <= a_step);
            }
            prev = Some(*s);
        }
        // Reached the synchronous (working) window.
        prop_assert!(saw_sync);

        // Returned home at rest.
        let (_, last, phase) = *trace.last().unwrap();
        prop_assert_eq!(phase, Phase::Waiting);
        prop_assert!((last.pos - home).abs() < 1e-5);
        prop_assert!(last.vel.abs() < 1e-5);
        prop_assert!(last.acc.abs() < 1e-5);
    }
}
