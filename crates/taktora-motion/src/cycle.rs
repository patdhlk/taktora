//! Per-cycle NC step over a [`CyclicFieldbus`] (`REQ_0861`, `REQ_0862`).
//!
//! [`NcCycle`] owns `N` [`AxisRuntime`]s, a [`CouplingTopology`], and a command
//! intake. Each cycle it brackets the bus exchange with `read_input` /
//! `write_output`, ticks every axis (masters before slaves so a slave reads its
//! master's *commanded* state same-cycle), and reacts to faults: an axis that
//! faults (statusword `Fault`/`FaultReactionActive`, or a stale input) drives
//! its engaged downstream subtree to `QuickStop` + `ErrorStop` (`REQ_0862`).
//!
//! Full command -> motion/token mapping is Task 12; the command drain here is a
//! minimal stub that routes by `axis_id` without crashing.
//!
//! ## Seam ↔ drive layout bridge
//!
//! [`AxisRuntime::tick`] reads sw/actual at offsets `7..13` and writes
//! cw/mode/target at `0..7` of a combined 13-byte image, via a base-0
//! [`MockDrive`]. The [`CyclicFieldbus`] seam is packed: `read_input` lands
//! sw+actual in `dst[..6]`, `write_output` reads cw+mode+target from `src[..7]`.
//! [`NcCycle`] owns a per-axis 13-byte `work` buffer and bridges them by reading
//! inputs into `work[7..13]` and writing outputs from `work[0..7]`. This
//! convention is mock-specific for Phase 4; the `EtherCAT` connector generalises
//! it in a later plan, so `step` is specialised to [`MockCyclicFieldbus`] here.

use taktora_cia402::Cia402Drive;
use taktora_cia402::state::{Cia402State, decode_state};
use taktora_cyclic_fieldbus::CyclicFieldbus;
use taktora_motion_proto as proto;

use crate::axis::AxisRuntime;
use crate::mock::{MockCyclicFieldbus, MockDrive, MockRouting};
use crate::scale::AxisScale;
use crate::topology::CouplingTopology;

/// Combined per-axis working buffer (`[cw:2][mode:1][target:4][sw:2][actual:4]`).
const BYTES_PER_AXIS: usize = 13;

/// `N`-axis NC runtime.
///
/// Owns the per-axis runtimes, the coupling topology, and the per-axis combined
/// working buffers that bridge the packed bus seam to the 13-byte image the
/// [`AxisRuntime`] / [`MockDrive`] operate on. Specialised to the mock fieldbus
/// seam ([`MockDrive`] / [`MockRouting`]) for Phase 4; the `EtherCAT` connector
/// generalises the buffer convention later.
#[derive(Debug)]
pub struct NcCycle<const N: usize> {
    axes: [AxisRuntime; N],
    topology: CouplingTopology<N>,
    /// Derived tick order (masters before slaves). A topological ordering of
    /// the declared master -> slave edges, computed by [`Self::precompute`] via
    /// Kahn's algorithm, so a slave reads its master's *commanded* state
    /// same-cycle (`REQ_0862`) regardless of declared index order. Defaults to
    /// identity until `precompute` runs.
    order: [usize; N],
    /// Per-axis direct declared master, recomputed by [`Self::precompute`].
    /// v1 supports a single master per slave.
    direct_master: [Option<usize>; N],
    /// Per-axis combined 13-byte working buffer (see [`BYTES_PER_AXIS`]).
    work: [[u8; BYTES_PER_AXIS]; N],
}

impl<const N: usize> NcCycle<N> {
    /// New NC runtime with per-axis drive-boundary `scales` (axis id = index).
    ///
    /// # Panics
    ///
    /// Panics if `N > 64` (the coupling topology supports at most 64 axes).
    #[must_use]
    pub fn new(scales: [AxisScale; N]) -> Self {
        // Index bounded by N <= 64; the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        let axes = core::array::from_fn(|i| AxisRuntime::new(i as proto::AxisId, scales[i]));
        Self {
            axes,
            topology: CouplingTopology::new(),
            order: core::array::from_fn(|i| i),
            direct_master: [None; N],
            work: [[0u8; BYTES_PER_AXIS]; N],
        }
    }

    /// Declare a static master -> slave coupling edge (forwards to topology).
    pub const fn add_edge(&mut self, master: u16, slave: u16) {
        self.topology.add_edge(master, slave);
    }

    /// Mark a declared edge engaged/disengaged (forwards to topology).
    pub const fn set_engaged(&mut self, master: u16, slave: u16, on: bool) {
        self.topology.set_engaged(master, slave, on);
    }

    /// Precompute transitive downstream sets and per-slave direct masters.
    /// Call once after all `add_edge`s.
    pub fn precompute(&mut self) {
        self.topology.precompute();
        // Resolve each slave's direct master via the declared adjacency: `m` is
        // a direct master of `s` iff `s` is in `m`'s downstream but in no
        // intermediate axis's downstream that `m` reaches. For v1's shallow
        // (depth-1, single-master) layouts, the direct master is simply the
        // lowest axis whose declared edge set contains `s`.
        for s in 0..N {
            self.direct_master[s] = (0..N).find(|&m| {
                m != s && {
                    #[allow(clippy::cast_possible_truncation)]
                    let edge = self.topology.declared_edge(m as u16, s as u16);
                    edge
                }
            });
        }

        // Derive the tick order as a topological ordering of the declared
        // master -> slave edges (Kahn's algorithm), so every master ticks
        // before all its slaves regardless of index order (`REQ_0862`
        // same-cycle following). N <= 64, so fixed scratch is cheap.
        let mut in_degree = [0u8; N];
        for (s, deg) in in_degree.iter_mut().enumerate() {
            for m in 0..N {
                #[allow(clippy::cast_possible_truncation)]
                if m != s && self.topology.declared_edge(m as u16, s as u16) {
                    *deg += 1;
                }
            }
        }
        let mut order = [0usize; N];
        let mut emitted = [false; N];
        let mut count = 0;
        // Repeatedly emit a lowest-index zero-in-degree axis, then relax the
        // in-degree of its declared slaves. Lowest-index tie-break keeps the
        // ordering deterministic and identity-stable for already-ordered graphs.
        loop {
            let next = (0..N).find(|&i| !emitted[i] && in_degree[i] == 0);
            let Some(m) = next else { break };
            emitted[m] = true;
            order[count] = m;
            count += 1;
            for s in 0..N {
                #[allow(clippy::cast_possible_truncation)]
                if self.topology.declared_edge(m as u16, s as u16) && !emitted[s] {
                    in_degree[s] -= 1;
                }
            }
        }
        if count == N {
            self.order = order;
        } else {
            // A cycle blocked a full ordering. Coupling graphs are
            // acyclic-expected (see topology docs); fall back to identity and
            // fail loudly in debug builds.
            debug_assert!(
                false,
                "coupling topology has a cycle; no valid tick order (emitted {count} of {N})"
            );
            self.order = core::array::from_fn(|i| i);
        }
    }

    /// The derived per-cycle tick order (masters before slaves). Test-internal
    /// accessor over the topological ordering computed by [`Self::precompute`].
    #[cfg(test)]
    pub(crate) const fn tick_order(&self) -> &[usize; N] {
        &self.order
    }

    /// Set axis `i`'s power target (`MC_Power`): `on` => `Enabled`.
    pub const fn request_power(&mut self, i: usize, on: bool) {
        self.axes[i].request_power(on);
    }

    /// Whether axis `i` reported `OperationEnabled` on its last tick.
    #[must_use]
    pub const fn is_enabled(&self, i: usize) -> bool {
        self.axes[i].is_enabled()
    }

    /// The last published status for axis `i`.
    #[must_use]
    pub const fn status_of(&self, i: usize) -> proto::AxisStatus {
        self.axes[i].status()
    }

    /// The token superseded on axis `i`'s most recent rising edge while still
    /// `Active` (Aborting buffer mode observation, `REQ_0855`). The single-slot
    /// [`proto::AxisStatus`] only carries the current token, so the aborted
    /// predecessor is surfaced through this accessor.
    #[must_use]
    pub const fn last_aborted_token_of(&self, i: usize) -> Option<proto::Token> {
        self.axes[i].last_aborted_token()
    }

    /// Axis `i`'s current `CiA` 402 power target (test/inspection helper).
    #[must_use]
    pub const fn power_target_of(&self, i: usize) -> taktora_cia402::PowerTarget {
        self.axes[i].power_target()
    }

    /// Run one NC cycle (`REQ_0861`, `REQ_0862`). In order:
    /// 1. drain `commands` (minimal stub — routes by `axis_id`, Task 12 maps);
    /// 2. per axis in declared order (masters first): `read_input` ->
    ///    freshness check -> `tick` (threading the master's commanded state) ->
    ///    `write_output`;
    /// 3. bus `exchange`;
    /// 4. fault detection (statusword `Fault`/`FaultReactionActive` or stale
    ///    input) -> drive each faulted axis's `engaged_downstream` subtree to
    ///    `QuickStop` + `ErrorStop`;
    /// 5. return per-axis status.
    ///
    /// `async` because the bus owns cycle timing via `exchange().await`
    /// (`REQ_0852`); callers in a sync context (e.g. tests) bracket it with
    /// `pollster::block_on`. The mock exchange is `Infallible`.
    pub async fn step(
        &mut self,
        bus: &mut MockCyclicFieldbus,
        commands: &[proto::AxisCommand],
        dt: f64,
    ) -> [proto::AxisStatus; N] {
        // 1. Drain commands: route by axis id, applying token-correlated
        //    command -> motion/power mapping with Aborting buffer mode
        //    (`REQ_0855`). `GearIn` consumes the axis's resolved direct master.
        for cmd in commands {
            let idx = cmd.axis_id as usize;
            if idx < N {
                let master = self.direct_master[idx];
                self.axes[idx].apply_command(cmd, master);
            }
        }

        // 2. Per axis, masters first.
        // `stale[i]` records a non-fresh input this cycle (counts as a fault).
        let mut stale = [false; N];
        for k in 0..N {
            let i = self.order[k];
            let routing = MockRouting {
                // Index bounded by N; cannot truncate at realistic axis counts.
                #[allow(clippy::cast_possible_truncation)]
                axis: i as u16,
            };
            // read_input -> work[7..13] (sw+actual where the drive reads them).
            let validity = bus.read_input(&routing, &mut self.work[i][7..13]);
            stale[i] = !validity.is_fresh();

            // Thread this slave's master commanded state (produced earlier this
            // cycle, since masters tick first under the declared order).
            let master = self.direct_master[i].map(|m| self.axes[m].commanded_state());

            let drive = MockDrive::for_axis(0); // base-0 over the 13-byte buffer
            self.axes[i].tick(&mut self.work[i], &drive, dt, master);

            // write_output <- work[0..7] (cw+mode+target).
            bus.write_output(&routing, &self.work[i][0..7]);
        }

        // 3. Bus exchange (the bus owns cycle timing; mock is Infallible).
        let _ = bus.exchange().await.unwrap_or_else(|e| match e {});

        // 4. Fault detection + engaged-downstream quickstop.
        //
        // Re-read each axis's statusword post-exchange so a fault injected this
        // cycle (visible only after `exchange`) is detected and propagated to
        // its engaged subtree before we publish status — making the engaged
        // slave deterministically reach `ErrorStop` in the same `step`.
        let mut faulted = [false; N];
        let drive0 = MockDrive::for_axis(0);
        for i in 0..N {
            let routing = MockRouting {
                #[allow(clippy::cast_possible_truncation)]
                axis: i as u16,
            };
            // Refresh sw into the working buffer to see the post-exchange state.
            bus.read_input(&routing, &mut self.work[i][7..13]);
            let sw = drive0.statusword(&self.work[i]);
            let drive_faulted = matches!(
                decode_state(sw),
                Cia402State::Fault | Cia402State::FaultReactionActive
            );
            faulted[i] = stale[i] || drive_faulted;
        }

        for (a, &is_faulted) in faulted.iter().enumerate() {
            if !is_faulted {
                continue;
            }
            // The faulted axis is itself in error: latch `ErrorStop` so it is
            // honest in the *same* `step` the fault is injected (its own `tick`
            // ran before `exchange`, so it published a pre-fault state this
            // cycle). `AxisRuntime::tick` would also publish `ErrorStop` from
            // the drive statusword on the next cycle (Change A), but latching
            // here makes the reaction same-cycle and coherent (`REQ_0861`).
            self.axes[a].force_error_stop();
            // Quickstop the engaged downstream subtree of the faulted axis `a`.
            #[allow(clippy::cast_possible_truncation)]
            for s in self.topology.engaged_downstream(a as u16) {
                let si = s as usize;
                self.axes[si].request_quick_stop();
                self.axes[si].force_error_stop();
            }
        }

        // 5. Publish per-axis status.
        core::array::from_fn(|i| self.axes[i].status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_scale() -> AxisScale {
        AxisScale {
            inc_per_unit: 1000.0,
            zero_offset: 0,
        }
    }

    /// Position of axis `a` in the tick order (panics if absent).
    fn pos<const N: usize>(order: &[usize; N], a: usize) -> usize {
        order.iter().position(|&x| x == a).expect("axis in order")
    }

    #[test]
    fn out_of_index_order_edge_ticks_master_before_slave() {
        // Declare master=1 -> slave=0: master index > slave index. Identity
        // order [0, 1] would tick the slave first, reading the master's stale
        // commanded state. The topological order must put 1 before 0.
        let mut nc = NcCycle::<2>::new([unit_scale(), unit_scale()]);
        nc.add_edge(1, 0);
        nc.precompute();

        let order = nc.tick_order();
        assert!(
            pos(order, 1) < pos(order, 0),
            "master 1 must tick before slave 0, got {order:?}"
        );
    }

    #[test]
    fn chain_orders_master_before_slave_transitively() {
        // Chain 2 -> 0 -> 1: must order 2 before 0 before 1, regardless of
        // index order.
        let mut nc = NcCycle::<3>::new([unit_scale(), unit_scale(), unit_scale()]);
        nc.add_edge(2, 0);
        nc.add_edge(0, 1);
        nc.precompute();

        let order = nc.tick_order();
        assert!(pos(order, 2) < pos(order, 0), "2 before 0, got {order:?}");
        assert!(pos(order, 0) < pos(order, 1), "0 before 1, got {order:?}");
    }

    #[test]
    fn engaged_fault_propagates_only_to_engaged_subtree() {
        // 3 axes; declare 0->1 and 0->2; engage ONLY 0->1.
        let mut nc = NcCycle::<3>::new([unit_scale(), unit_scale(), unit_scale()]);
        nc.add_edge(0, 1);
        nc.add_edge(0, 2);
        nc.set_engaged(0, 1, true);
        nc.precompute();

        let mut bus = MockCyclicFieldbus::new(3);
        for i in 0..3 {
            nc.request_power(i, true);
        }

        let dt = 0.002;
        for _ in 0..16 {
            pollster::block_on(nc.step(&mut bus, &[], dt));
        }
        assert!((0..3).all(|i| nc.is_enabled(i)), "all axes should enable");

        // Fault axis 0 (the master) and run one step.
        bus.inject_fault(0);
        pollster::block_on(nc.step(&mut bus, &[], dt));

        // Engaged slave (1) reaches ErrorStop AND its power target is QuickStop.
        assert_eq!(nc.status_of(1).state, proto::AxisState::ErrorStop);
        assert_eq!(
            nc.power_target_of(1),
            taktora_cia402::PowerTarget::QuickStop
        );

        // Disengaged sibling (2) is untouched.
        assert_ne!(nc.status_of(2).state, proto::AxisState::ErrorStop);
        assert_ne!(
            nc.power_target_of(2),
            taktora_cia402::PowerTarget::QuickStop
        );
    }

    /// Drive axis 0 up to `OperationEnabled`, running empty `step`s.
    fn enable_axis0(nc: &mut NcCycle<1>, bus: &mut MockCyclicFieldbus, dt: f64) {
        nc.request_power(0, true);
        for _ in 0..16 {
            pollster::block_on(nc.step(bus, &[], dt));
            if nc.is_enabled(0) {
                break;
            }
        }
        assert!(nc.is_enabled(0), "axis 0 should enable");
    }

    fn vel_params(v: f64, a: f64) -> proto::CommandParams {
        proto::CommandParams {
            target_pos: 0.0,
            velocity: v,
            accel: a,
            jerk: 0.0,
        }
    }

    #[test]
    fn token_correlation_aborting_and_infeasible() {
        let mut nc = NcCycle::<1>::new([unit_scale()]);
        nc.precompute();
        let mut bus = MockCyclicFieldbus::new(1);
        let dt = 0.002;
        enable_axis0(&mut nc, &mut bus, dt);

        // 1. MoveVelocity, token 1 -> Active + ContinuousMotion.
        let move_vel = proto::AxisCommand {
            axis_id: 0,
            token: 1,
            kind: proto::CommandKind::MoveVelocity,
            params: vel_params(25.0, 100.0),
        };
        pollster::block_on(nc.step(&mut bus, &[move_vel], dt));
        let s = nc.status_of(0);
        assert_eq!(s.last_token, 1, "token 1 latched");
        assert_eq!(s.token_state, proto::TokenState::Active, "token 1 active");
        assert_eq!(
            s.state,
            proto::AxisState::ContinuousMotion,
            "velocity move -> ContinuousMotion"
        );

        // 2. Stop, token 2 -> token 2 Active, token 1 reported Aborted (supersede).
        let stop = proto::AxisCommand {
            axis_id: 0,
            token: 2,
            kind: proto::CommandKind::Stop,
            params: vel_params(0.0, 100.0),
        };
        pollster::block_on(nc.step(&mut bus, &[stop], dt));
        let s = nc.status_of(0);
        assert_eq!(s.last_token, 2, "token 2 now latched");
        assert_eq!(s.token_state, proto::TokenState::Active, "token 2 active");
        assert_eq!(
            nc.last_aborted_token_of(0),
            Some(1),
            "superseded token 1 aborted (Aborting buffer mode)"
        );

        // 3. Infeasible FlyingSaw, token 3 -> Error, NO fault, motion unchanged.
        let state_before = nc.status_of(0).state;
        let saw = proto::AxisCommand {
            axis_id: 0,
            token: 3,
            kind: proto::CommandKind::FlyingSaw,
            params: vel_params(0.0, 100.0),
        };
        pollster::block_on(nc.step(&mut bus, &[saw], dt));
        let s = nc.status_of(0);
        assert_eq!(s.last_token, 3, "token 3 latched");
        assert_eq!(
            s.token_state,
            proto::TokenState::Error,
            "infeasible FlyingSaw -> Error"
        );
        assert_ne!(
            s.state,
            proto::AxisState::ErrorStop,
            "infeasible command must NOT fault the axis"
        );
        assert_eq!(
            s.state, state_before,
            "active motion unchanged by rejected command"
        );
    }
}
