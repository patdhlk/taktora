# ethercat-stepper

Drives a Beckhoff **EL7047** stepper terminal from **EL1008** button
presses, with the **EL2004** acting as a drive-status lamp — all on one
EtherCAT bus behind an EK1100 coupler, via
`taktora-connector-ethercat`'s `EthercrabBusDriver`. Each button press
fires a fixed-size **relative** index move; the terminal runs the
trapezoid itself.

The control loop runs at 10 ms: it reads the EL1008 inputs, decides the
next EL7047 control word with an edge-triggered state machine
(`src/control.rs`), writes it into the ESI-codegen-generated EL7047
device through a thin domain adapter (`src/el7047_adapter.rs`), and
updates the EL2004 status lamp. The wire codec is **generated**, not
hand-written — see *Generated EL7047 codec* below.

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

## Two-layer codegen: `esi/*.xml` + `network.yaml`

`build.rs` runs **two** independent code-generation passes; neither
produces hand-written codec or wiring code:

1. **Per-device ESI codegen** (`taktora-ethercat-esi-build`) reads every
   `esi/*.xml` and emits `generated::*` — one typed struct per terminal
   (`EL7047`, `EL1008`, `EL2004`), each with a joint `*OpMode` enum
   over its selectable PDO assignments, plus `encode_outputs` /
   `decode_inputs` for every mode.

2. **Per-bus netcfg codegen** (`taktora-ethercat-netcfg-build`) reads
   `network.yaml` and emits `generated_net::*` — the `PDO_MAP` for the
   full bus, `EXPECTED_IDENTITIES` (vendor/product IDs in bus order,
   currently logged at startup; runtime enforcement is not yet wired),
   and per-channel routing consts (`ETHERCAT_EL1008_INPUTS`,
   `ETHERCAT_EL2004_OUTPUTS`, `ETHERCAT_EL7047_CONTROL`,
   `ETHERCAT_EL7047_STATUS`) that carry byte-offset and bit-length into
   the shared PDO image.

`main.rs` consumes both: `generated::EL7047 { mode: EL7047OpMode::PositioningInterface(…) }`
for encoding/decoding, and `generated_net::PDO_MAP` / the routing consts
for image slicing.

## Generated EL7047 codec

The wire codec is produced by the ESI device-codegen toolchain from
[`esi/beckhoff_el7047.xml`](esi/beckhoff_el7047.xml), not hand-written.
The EL7047 declares several selectable PDO assignments, so the generated
device models them as a joint `EL7047OpMode` enum — one variant per
assignment — and the device is `struct EL7047 { mode: EL7047OpMode }`
(issue #70). The example pins the **`PositioningInterface`** variant
(declared in `network.yaml` via `op_mode: "Positioning interface"` — see
*Two-layer codegen* above):

- **Domain types** (`src/el7047_domain.rs`) — `El7047Control` /
  `El7047Status`, the example's hardware-agnostic control surface that
  `src/control.rs` reasons about.
- **Adapter** (`src/el7047_adapter.rs`) — maps those domain types onto
  the generated `PositioningInterface` variant's typed fields
  (`enc_control` / `stm_control` / `pos_control`, etc.). The actual
  encode/decode is the generated `EL7047::encode_outputs` /
  `decode_inputs`.
- **PDO map** — the bus-wide `PDO_MAP` (including the EL7047's
  `0x1C12`/`0x1C13` assignment lists) is emitted by the netcfg codegen
  from `network.yaml`; the ESI codegen drives the codec. Both layers stay
  in sync because `network.yaml`'s `op_mode` selects the same ESI
  AlternativeSmMapping variant that the codec was generated from.

EL1008 and EL2004 are generated the same way (each a single-variant
`Default` mode). There is no hand-written PDO codec in this example.

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
> starting at `0x1000` in bus-scan order. The `devices:` order in
> `network.yaml` defines bus order: EK1100 = `0x1000` (coupler, no
> PDI), **EL1008 = `0x1001`**, **EL2004 = `0x1002`**, **EL7047 =
> `0x1003`** — i.e. physical left-to-right order right of the coupler.
> The routing consts and PDO map are generated from that order. **If
> your physical layout differs, reorder the `devices:` list in
> `network.yaml`** (do not edit code) and rebuild — the generated
> addresses and image offsets update automatically. Run with
> `RUST_LOG=ethercrab=debug` to see the scanned addresses.

## Button map (EL1008)

Channels are Lsb0 (`ch1` = bit 0). Index moves are edge-triggered;
emergency-stop and the jog buttons are level-triggered (held):

| Channel | Bit | Function | Trigger |
|---|---|---|---|
| ch1 | 0 | Index **+** (relative move by `+step`) | rising edge |
| ch2 | 1 | Index **−** (relative move by `−step`) | rising edge |
| ch3 | 2 | **Emergency stop** | level (held) |
| ch4 | 3 | **Fault reset** | rising edge |
| ch5 | 4 | **Jog +** (endless, drive toward the block) | level (held) |
| ch6 | 5 | **Jog −** (endless) | level (held) |
| ch7 | 6 | **Set zero** (datum here: load 0 into the position counter) | level (held) |

Behaviour notes:

- **Auto-enable.** No button enables the drive; Enable is held set
  whenever the bus is healthy and ch3 is not asserted.
- **Busy lockout.** While a move is running (POS Status `Busy`), new
  index presses are ignored — one move per press, no queueing.
- **Execute hold.** On the firing edge, Execute is driven high and
  **held for the whole move** — until the drive has reported `Busy` and
  then returned to not-`Busy` (move complete). Dropping Execute early
  aborts the travel on the EL7047, so it must be held throughout.
  Emergency-stop or loss of connector health forces Execute low
  immediately — safe-state wins.
- **Jog** (ch5/ch6). Hold to run the motor continuously in one
  direction (Start type *Endless plus/minus*); release to stop. Jog
  **overrides** the index moves while held. It also stops automatically
  on a **motor stall** — e.g. when the carriage reaches a hard block —
  so you can drive onto a block without grinding. Holding both jog
  buttons is ambiguous and stops the drive. (A stall may latch the STM
  error; press ch4 to reset.)
- **Set zero** (ch7). Loads `0` into the position counter at the current
  location via the ENC Control PDO ("Set counter"), giving a readable
  datum. No motion occurs while it is held. Because index moves are
  relative, this is a convenience for a clean reference, not a
  prerequisite for motion.
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
are **mA**. They live in `network.yaml` under `el7047.startup_sdos`:

```yaml
startup_sdos:
  - { index: 0x8010, subindex: 0x01, type: u16, value: 1800 }  # max current, mA
  - { index: 0x8010, subindex: 0x02, type: u16, value: 900 }   # standby current, mA
```

To match a different motor, edit those `value:` fields in `network.yaml`
and rebuild — no code change required. **Do not** run with limits above
your motor's rating.

## Safe state on fault / cable pull

Two layers stop the motor if the master goes away:

- **Hardware (SM watchdog).** The EL7047 and EL2004 are output-bearing,
  so the PDO map programs a **50 ms** SyncManager watchdog on each
  (set via `sm_watchdog_timeout_ms: 50` in `network.yaml` — AOU_0016's
  FTTI/2 bound, programmed and read-back-verified by the driver at
  bring-up). On a master crash or cable pull the terminal stops
  receiving process data and drops to safe state — the motor stops
  within ~50 ms with no software involvement.
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
  `0x1001/0x1002/0x1003` assumption — reorder the `devices:` list in
  `network.yaml` to match the physical bus, then rebuild.

## What this shows

- The Beckhoff positioning interface end-to-end over the workspace
  connector stack: the ESI-codegen-generated EL7047 driving its
  selectable `PositioningInterface` PDO assignment (chosen in
  `network.yaml` via `op_mode: "Positioning interface"`) via a thin
  domain adapter (`src/el7047_adapter.rs`), an edge-triggered
  button→motion state machine (`src/control.rs`), and the connector's
  reader/writer channels carrying fixed-size images.
- Declarative bus configuration in `network.yaml` compiled to
  `generated_net::PDO_MAP` and routing consts by
  `taktora-ethercat-netcfg-build` — topology, PDO assignment, SM
  watchdogs, working-counter, and startup SDOs declared once and never
  hand-coded.
- Operator-declared startup SDOs (motor current limits) in
  `network.yaml` applied in PRE-OP before PDO assignment (`REQ_0853`).
- The 50 ms FTTI/2 safe-state SM watchdog on the output-bearing
  terminals declared via `sm_watchdog_timeout_ms:` in `network.yaml`
  (AOU_0016 / `REQ_0846`).
- All three terminals codegen-typed on the same `RawImageCodec` channel
  surface (`EL1008::decode_inputs`, `EL2004::encode_outputs`, and the
  EL7047's per-mode `decode_inputs`/`encode_outputs`), image offsets
  carried by the `generated_net` routing consts.
