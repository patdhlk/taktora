# taktora-motion-core

Allocation-free, `no_std` real-time motion **trajectory core** for the
[taktora](https://taktora.eu) runtime.

This crate is the pure algorithmic layer of taktora's motion stack: it computes
commanded axis setpoints as bounded, allocation-free, panic-free functions of
`(dt, master)`. It owns no I/O, no threads, no shared mutable state, and no
dependency on the executor or iceoryx2 — that machinery lives one layer up in
`taktora-motion`. The intended deployment is a **setpoint generator** feeding
CiA 402 drives in Cyclic Synchronous Position (CSP) mode, where the drive closes
its own velocity and current loops; taktora only produces the commanded
position each cycle.

## What's here

The unit of computation is an [`Axis`](src/group.rs) that owns a
[`Motion`](src/motion.rs) generator. An [`AxisGroup<N>`](src/group.rs) ticks a
fixed array of axes in a build-time topologically-sorted order (masters before
slaves), so electronic coupling is same-cycle coherent.

The `Motion` enum is monomorphized — **no `Box<dyn>`, no vtable on the hot
path** — so dispatch stays bounded and allocation-free.

### Slice scope (v1)

| Variant      | Status | Notes                                               |
| ------------ | ------ | --------------------------------------------------- |
| `Idle`       | ✅      | hold position                                       |
| `Velocity`   | ✅      | constant-velocity jog with bounded accel ramp; also drives the virtual master |
| `Trapezoid`  | ✅      | point-to-point; triangular/trapezoidal via `sqrt`   |
| `Gear`       | ✅      | electronic gearing — `slave = ratio · master`       |
| `SCurve`     | ⏳      | deferred — jerk-limited 7-segment                   |
| `Cam`        | ⏳      | deferred — pre-allocated table + motion laws        |
| `FlyingSaw`  | ⏳      | deferred — quintic sync-on / feasibility / return   |
| `Superimposed` | ⏳    | deferred                                            |

A **virtual master is just an `Axis`** with no upstream master, typically
running a `Velocity` profile; slaves couple to its set-state.

## Conventions

- `#![no_std]`, no `alloc`. `f64` engineering units (revolutions / mm / deg);
  increments↔units scaling is the glue layer's job, not this crate's.
- `dt` is passed per tick (seconds). The integrator tolerates a late cycle;
  pass the nominal cycle period for bit-determinism.
- Modulo (endless rotary) wrap is applied once, in `AxisGroup::tick`, after the
  generator returns — never smeared through the `Motion` arms.

See `spec/` in the taktora repository for the requirement/architecture needs
that govern this crate.
