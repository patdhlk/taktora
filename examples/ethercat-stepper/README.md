# ethercat-stepper

Drives a Beckhoff **EL7047** stepper terminal from **EL1008** button
presses, with the **EL2004** acting as a drive-status lamp — all on one
EtherCAT bus behind an EK1100 coupler, via
`taktora-connector-ethercat`'s `EthercrabBusDriver`. Each button press
fires a fixed-size **relative** index move; the terminal runs the
trapezoid itself.

The control loop runs at 10 ms: it reads the EL1008 inputs, decides the
next EL7047 control word with an edge-triggered state machine
(`src/control.rs`), encodes the positioning-interface image
(`src/el7047.rs`), and updates the EL2004 status lamp.

## Motion model: the Beckhoff positioning interface (NOT CiA 402)

The EL7047 does **not** speak CiA 402 (no `0x6040`/`0x6041` control- and
status-words, no operation modes). It uses Beckhoff's proprietary
**positioning interface**, built from two function blocks:

- **STM** (stepper-motor) Control `0x7010` / Status `0x6010` — enable,
  reset, ready, error, motor-stall.
- **POS** (positioning) Control `0x7020` / Status `0x6020` — execute,
  emergency-stop, busy, in-target, plus the target/velocity/ramp move
  parameters.

Motion is **one relative move per button press**: the example writes a
Target position delta, Velocity, ramps and Start type = **Relative**
(`2`) into the POS Control image, then pulses **Execute** 0→1. The
*terminal* runs the trapezoid to completion on its own — the master only
kicks it off.

Enabling is automatic: while the connector is healthy and no
emergency-stop is asserted, the controller holds the STM **Enable** bit
set, and the drive walks itself to **Ready** at startup. There is no
homing or absolute reference — positions are relative.

## Hardware required

- Beckhoff **EK1100** bus coupler.
- Beckhoff **EL1008** 8-channel 24 V digital input terminal (buttons).
- Beckhoff **EL2004** 4-channel digital output terminal (status lamp).
- Beckhoff **EL7047** stepper-motor terminal.
- Bus end cap (EL9011 ships with the EK1100).
- **24 V DC logic supply** on the EK1100's `Us` / `Up` terminals.
- A **motor / field supply** wired to the EL7047's power contacts — the
  EL7047 will not move the motor on the bus logic supply alone.
- A **stepper motor** wired to the EL7047. The example's startup SDOs
  assume a motor rated around **1.8 A** (see *Motor current* below);
  set them to match your motor before energising.
- Cat5e (or better) Ethernet from your host's wired NIC to the EK1100's
  **IN (X1)** port. Leave OUT (X2) empty unless daisy-chaining.

> **Topology assumption.** The example pins each terminal to its
> EtherCAT configured station address. `ethercrab` auto-assigns these
> starting at `0x1000` in bus-scan order, so with the canonical layout:
> EK1100 = `0x1000`, **EL1008 = `0x1001`**, **EL2004 = `0x1002`**,
> **EL7047 = `0x1003`** (i.e. physical left-to-right order right of the
> coupler). The driver matches on the configured station address, not a
> topology index. If your physical order differs (or you have extra
> terminals in between), edit the `SUBDEV_EL1008` / `SUBDEV_EL2004` /
> `SUBDEV_EL7047` constants in `src/main.rs` — **if the EL7047 address
> is wrong, every routing and PDO-map entry for it is off.** Run with
> `RUST_LOG=ethercrab=debug` to see the scanned addresses.

## Button map (EL1008)

Channels are Lsb0 (`ch1` = bit 0). The controller is edge-triggered
except for emergency-stop, which is level-triggered:

| Channel | Bit | Function | Trigger |
|---|---|---|---|
| ch1 | 0 | Index **+** (relative move by `+step`) | rising edge |
| ch2 | 1 | Index **−** (relative move by `−step`) | rising edge |
| ch3 | 2 | **Emergency stop** | level (held) |
| ch4 | 3 | **Fault reset** | rising edge |

Behaviour notes:

- **Auto-enable.** No button enables the drive; Enable is held set
  whenever the bus is healthy and ch3 is not asserted.
- **Busy lockout.** While a move is running (POS Status `Busy`), new
  index presses are ignored — one move per press, no queueing.
- **Execute hold.** On the firing edge, Execute is driven high and
  **held** until the drive acknowledges with `Busy`, then dropped so it
  is armed for the next press. Emergency-stop or loss of connector
  health forces Execute (and the latch) low immediately — safe-state
  wins.
- **Emergency stop** (ch3) clears Enable and asserts the POS Control
  emergency-stop bit for as long as the button is held.

## Status lamp (EL2004)

- **ch1** — lit while the drive is **Ready** *and* the connector is
  healthy (`Up`).
- **ch2** — **blinks at ~2 Hz** while a fault is latched (STM Status
  error). The blink half-period is 250 ms (25 control cycles), so it is
  visible rather than a dim flicker.

## Motor current

Set via two startup SDOs written in PRE-OP, before the PDO mapping is
committed (the connector's `with_startup_sdos` path, `REQ_0853`). Units
are **mA**:

- `0x8010:01` — **maximum** current = `1800` mA.
- `0x8010:02` — **standby** (reduced) current = `900` mA.

To match a different motor, edit `EL7047_STARTUP` in `src/main.rs`.
**Do not** run with limits above your motor's rating.

## Safe state on fault / cable pull

Two layers stop the motor if the master goes away:

- **Hardware (SM watchdog).** The EL7047 and EL2004 are output-bearing,
  so the PDO map programs a **50 ms** SyncManager watchdog on each
  (`SmWatchdog::from_timeout_us(50_000)` — AOU_0016's FTTI/2 bound,
  programmed and read-back-verified by the driver at bring-up). On a
  master crash or cable pull the terminal stops receiving process data
  and drops to safe state — the motor stops within ~50 ms with no
  software involvement.
- **Software reaction.** When the connector reports `Degraded` or
  `Down`, the control loop clears Enable and refuses new moves. This is
  belt-and-braces on top of the hardware watchdog.

## Build + run

EtherCAT masters need raw socket access. Build release, then run with
`sudo` (or grant `cap_net_raw` to the binary as in `ethercat-real-bus`):

```bash
cargo build --release
sudo ./target/release/ethercat-stepper \
  --nic eth0 --step 3200 --velocity 1000 --accel 1000 --decel 1000
```

Flags (all optional; defaults shown):

| Flag | Default | Meaning |
|---|---|---|
| `--nic` | `eth0` | NIC the EK1100 is wired to. |
| `--step` | `3200` | Increments per index press (relative move delta). |
| `--velocity` | `1000` | Move velocity (raw POS-interface units). |
| `--accel` | `1000` | Acceleration ramp (raw POS-interface units). |
| `--decel` | `1000` | Deceleration ramp (raw POS-interface units). |

Units are EL7047 *increments*: with the typical 64× microstepping the
EL7047 reports ~12800 increments per revolution, so `--step 3200` is
roughly a quarter turn per press. **Velocity and the ramps are raw
positioning-interface values bounded by object `0x8020`** — their
scaling is device-defined, so start small and tune up on the bench
rather than guessing a physical speed.

> **Raspberry Pi note.** The Pi is the canonical target. Bring the
> wired NIC UP without an IP (EtherCAT is Layer 2 only), then run with
> `sudo` — and the `sudo` password is required. Missing `CAP_NET_RAW`
> (no `sudo`, or `setcap` not applied after a rebuild) shows up as a
> **silent hang at `Connecting`**, not an error. See
> `ethercat-real-bus/README.md` for the full host-setup recipe (the NIC
> and capability setup are identical).

## What you should see

Bring-up, then one status line per EL7047 status change (actual
position, ready / in-target / busy / error / stall), and health
transitions on stderr:

```
ethercat connector health at startup: Connecting
t=+   540ms  ethercat health: Connecting -> Up
t=+   780ms  pos=       0  ready=1 in_target=1 busy=0 error=0 stall=0
t=+  3120ms  pos=       0  ready=1 in_target=0 busy=1 error=0 stall=0
t=+  4050ms  pos=    3200  ready=1 in_target=1 busy=0 error=0 stall=0
```

Press ch1, watch `pos` advance by `--step`; press ch2 to go back.

## Building this example

Like the other examples, building against in-tree crate changes (rather
than the published versions) uses the workspace-local patch toggle —
flip the `[patch.crates-io]` block in `Cargo.toml`:

```bash
scripts/examples-local.sh on        # uncomment the patch block
cd examples/ethercat-stepper && cargo run --release -- --nic eth0
scripts/examples-local.sh off       # restore committed state
```

CI refuses to proceed if any example reports `on` — never commit while
the toggle is active. From a clean checkout the trimmed ESI fixtures in
[`esi/`](esi/) let the example build standalone with no extra files.

## Troubleshooting

- **Stuck in `Connecting` (silent).** Almost always raw-socket
  permissions on the Pi — run with `sudo` (password required) or apply
  `setcap cap_net_raw,cap_net_admin=eip` to the binary. Otherwise check
  cable, 24 V on the EK1100, and the NIC name.
- **Motor does not move but the drive reports Ready.** Check the
  EL7047's motor/field supply is wired and powered — the logic supply
  alone enables the terminal but cannot drive the motor.
- **`error=1` / `stall=1` on the status line.** A fault is latched (the
  EL2004 ch2 blinks). Press ch4 to reset once the cause is cleared.
- **Wrong terminal addresses.** If the EL7047 image looks wrong or
  bring-up fails on the mapping, your physical order differs from the
  `0x1001/0x1002/0x1003` assumption — adjust the `SUBDEV_*` constants.

## What this shows

- The Beckhoff positioning interface end-to-end over the workspace
  connector stack: a hand-written PDO codec (`src/el7047.rs`), an
  edge-triggered button→motion state machine (`src/control.rs`), and
  the connector's reader/writer channels carrying fixed-size images.
- `SubDeviceMap::with_startup_sdos(...)` — operator-declared startup
  SDOs (motor current) applied in PRE-OP before PDO assignment
  (`REQ_0853`).
- `SubDeviceMap::with_sm_watchdog(...)` — the 50 ms FTTI/2 safe-state
  watchdog on the output-bearing terminals (AOU_0016 / `REQ_0846`).
- Codegen-typed EL1008/EL2004 (`EL1008::decode_inputs` /
  `EL2004::encode_outputs`) sitting alongside the hand-written EL7047
  codec on the same `RawImageCodec` channel surface.
