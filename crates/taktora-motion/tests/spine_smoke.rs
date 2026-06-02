//! End-to-end Phase-4 integration smoke test over the virtual drive
//! (`FEAT_0092`). Proves Phases 1-4 compose on the host: two coupled axes go
//! through bring-up, a master discrete move, a slave gearing in and tracking
//! the master, a fault driving both axes safe, and a reset + bumpless re-enable.
//!
//! This is a separate integration crate, so it exercises only the `pub` API of
//! `taktora-motion` plus `taktora-motion-proto`. The async `step` is driven with
//! `pollster::block_on` (allocation is not measured here — that is the
//! `taktora-motion-tests` no-alloc guard's job).

use taktora_motion::cycle::NcCycle;
use taktora_motion::mock::MockCyclicFieldbus;
use taktora_motion::scale::AxisScale;
use taktora_motion_proto as proto;

/// 1000 increments per engineering unit, no offset.
const fn unit_scale() -> AxisScale {
    AxisScale {
        inc_per_unit: 1000.0,
        zero_offset: 0,
    }
}

/// Fieldbus cycle time (2 ms).
const DT: f64 = 0.002;
/// Gear ratio used in the synchronized-motion stage.
const RATIO: f64 = 2.0;
/// Tolerance for commanded-position comparisons (engineering units).
const EPS: f64 = 1e-3;

/// Block one NC cycle.
fn step<const N: usize>(
    nc: &mut NcCycle<N>,
    bus: &mut MockCyclicFieldbus,
    cmds: &[proto::AxisCommand],
) -> [proto::AxisStatus; N] {
    pollster::block_on(nc.step(bus, cmds, DT))
}

const fn command(
    axis_id: proto::AxisId,
    token: proto::Token,
    kind: proto::CommandKind,
    params: proto::CommandParams,
) -> proto::AxisCommand {
    proto::AxisCommand {
        axis_id,
        token,
        kind,
        params,
    }
}

/// Stage 1: bring both coupled axes up to enabled + idle `Standstill`.
fn bring_up(nc: &mut NcCycle<2>, bus: &mut MockCyclicFieldbus) {
    nc.request_power(0, true);
    nc.request_power(1, true);
    for _ in 0..16 {
        step(nc, bus, &[]);
        if nc.is_enabled(0) && nc.is_enabled(1) {
            break;
        }
    }
    assert!(nc.is_enabled(0), "master should enable");
    assert!(nc.is_enabled(1), "slave should enable");
    assert_eq!(
        nc.status_of(0).state,
        proto::AxisState::Standstill,
        "enabled+idle master is Standstill"
    );
    assert_eq!(
        nc.status_of(1).state,
        proto::AxisState::Standstill,
        "enabled+idle slave is Standstill"
    );
}

/// Stage 2: master executes an absolute S-curve move and returns to Standstill.
/// Returns the commanded position the master settled at.
fn master_discrete_move(nc: &mut NcCycle<2>, bus: &mut MockCyclicFieldbus) -> f64 {
    let start_cmd = nc.status_of(0).cmd_pos;
    let target = start_cmd + 20.0;
    let move_abs = command(
        0,
        1,
        proto::CommandKind::MoveAbsolute,
        proto::CommandParams {
            target_pos: target,
            velocity: 50.0,
            accel: 500.0,
            jerk: 5000.0, // jerk > 0 -> SCurve
        },
    );
    step(nc, bus, &[move_abs]);
    assert_eq!(
        nc.status_of(0).state,
        proto::AxisState::DiscreteMotion,
        "master executing a point-to-point move is DiscreteMotion"
    );
    assert_eq!(
        nc.status_of(0).token_state,
        proto::TokenState::Active,
        "move token is Active while running"
    );

    let mut completed = false;
    for _ in 0..600 {
        step(nc, bus, &[move_abs]);
        if nc.status_of(0).token_state == proto::TokenState::Done {
            completed = true;
            break;
        }
    }
    assert!(completed, "discrete move should complete -> token Done");
    let end_cmd = nc.status_of(0).cmd_pos;
    assert_eq!(
        nc.status_of(0).state,
        proto::AxisState::Standstill,
        "master returns to Standstill after the move completes"
    );
    assert!(
        (end_cmd - target).abs() < EPS,
        "master commanded position reached the target (got {end_cmd}, want {target})"
    );
    assert!(
        end_cmd > start_cmd + 19.0,
        "commanded position actually moved toward the target"
    );
    end_cmd
}

/// Stage 3: slave gears in and tracks the master at `RATIO`. Engages the
/// coupling (so a later master fault propagates) and returns the `GearIn`
/// command so the fault stage can keep it issued.
fn slave_gears_in(
    nc: &mut NcCycle<2>,
    bus: &mut MockCyclicFieldbus,
    master_at: f64,
) -> proto::AxisCommand {
    let gear_in = command(
        1,
        10,
        proto::CommandKind::GearIn,
        proto::CommandParams {
            target_pos: RATIO, // GearIn arm reads the ratio from target_pos
            velocity: 0.0,
            accel: 0.0,
            jerk: 0.0,
        },
    );
    nc.set_engaged(0, 1, true);
    step(nc, bus, &[gear_in]);
    assert_eq!(
        nc.status_of(1).state,
        proto::AxisState::SynchronizedMotion,
        "geared slave is SynchronizedMotion"
    );

    let move_master2 = command(
        0,
        2,
        proto::CommandKind::MoveAbsolute,
        proto::CommandParams {
            target_pos: master_at + 10.0,
            velocity: 50.0,
            accel: 500.0,
            jerk: 5000.0,
        },
    );
    step(nc, bus, &[move_master2, gear_in]);
    for _ in 0..600 {
        step(nc, bus, &[gear_in]);
        if nc.status_of(0).token_state == proto::TokenState::Done {
            break;
        }
    }
    let master_cmd = nc.status_of(0).cmd_pos;
    let slave_cmd = nc.status_of(1).cmd_pos;
    assert_eq!(
        nc.status_of(1).state,
        proto::AxisState::SynchronizedMotion,
        "slave stays SynchronizedMotion while following"
    );
    assert!(
        RATIO.mul_add(-master_cmd, slave_cmd).abs() < EPS,
        "slave commanded ({slave_cmd}) tracks ratio*master ({})",
        RATIO * master_cmd
    );
    // The slave must actually have been carried somewhere by the gear, so the
    // bumpless re-enable check below is meaningful (not trivially 0 ~= 0).
    assert!(
        slave_cmd.abs() > 1.0,
        "geared slave commanded position is non-trivial (got {slave_cmd})"
    );
    gear_in
}

/// Stage 4: fault the master; both axes must end in `ErrorStop` and the engaged
/// slave's power target must be `QuickStop`. Returns the master/slave actuals at
/// the fault point for the bumpless re-enable check.
fn fault_both_safe(
    nc: &mut NcCycle<2>,
    bus: &mut MockCyclicFieldbus,
    gear_in: proto::AxisCommand,
) -> (f64, f64) {
    bus.inject_fault(0);
    step(nc, bus, &[gear_in]);
    assert_eq!(
        nc.status_of(0).state,
        proto::AxisState::ErrorStop,
        "faulted master is ErrorStop (drive statusword decodes to Fault)"
    );
    assert_eq!(
        nc.status_of(1).state,
        proto::AxisState::ErrorStop,
        "engaged-downstream slave is driven to ErrorStop"
    );
    assert_eq!(
        nc.power_target_of(1),
        taktora_cia402::PowerTarget::QuickStop,
        "slave's power target is QuickStop (safe-state reaction)"
    );
    (nc.status_of(0).actual_pos, nc.status_of(1).actual_pos)
}

/// Stage 5: clear the fault, reset + re-enable both axes, and assert a bumpless
/// return to `Standstill` (commanded ~= actual at re-enable, no lurch to 0).
fn reset_and_reenable(
    nc: &mut NcCycle<2>,
    bus: &mut MockCyclicFieldbus,
    master_actual: f64,
    slave_actual: f64,
) {
    bus.clear_fault(0);
    let reset0 = command(
        0,
        100,
        proto::CommandKind::Reset,
        proto::CommandParams {
            target_pos: 0.0,
            velocity: 0.0,
            accel: 0.0,
            jerk: 0.0,
        },
    );
    let reset1 = command(
        1,
        101,
        proto::CommandKind::Reset,
        proto::CommandParams {
            target_pos: 0.0,
            velocity: 0.0,
            accel: 0.0,
            jerk: 0.0,
        },
    );
    nc.request_power(0, true);
    nc.request_power(1, true);

    let mut reenabled = false;
    for i in 0..32 {
        let cmds: &[proto::AxisCommand] = if i == 0 { &[reset0, reset1] } else { &[] };
        step(nc, bus, cmds);
        if nc.is_enabled(0) && nc.is_enabled(1) {
            reenabled = true;
            break;
        }
    }
    assert!(reenabled, "both axes re-enable after clear_fault + Reset");
    assert_eq!(
        nc.status_of(0).state,
        proto::AxisState::Standstill,
        "master back to Standstill after recovery"
    );
    assert_eq!(
        nc.status_of(1).state,
        proto::AxisState::Standstill,
        "slave back to Standstill after recovery"
    );

    let master_cmd = nc.status_of(0).cmd_pos;
    let slave_cmd = nc.status_of(1).cmd_pos;
    assert!(
        (master_cmd - master_actual).abs() < EPS,
        "master re-enable is bumpless (cmd {master_cmd} ~= actual {master_actual})"
    );
    assert!(
        (slave_cmd - slave_actual).abs() < EPS,
        "slave re-enable is bumpless (cmd {slave_cmd} ~= actual {slave_actual})"
    );
}

#[test]
fn spine_smoke_two_coupled_axes() {
    // Master = axis 0, slave = axis 1, geared 0 -> 1.
    let mut nc = NcCycle::<2>::new([unit_scale(), unit_scale()]);
    nc.add_edge(0, 1);
    nc.precompute();
    let mut bus = MockCyclicFieldbus::new(2);

    bring_up(&mut nc, &mut bus);
    let master_at = master_discrete_move(&mut nc, &mut bus);
    let gear_in = slave_gears_in(&mut nc, &mut bus, master_at);
    let (master_actual, slave_actual) = fault_both_safe(&mut nc, &mut bus, gear_in);
    reset_and_reenable(&mut nc, &mut bus, master_actual, slave_actual);
}
