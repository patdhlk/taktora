# ethercat-wago-coupler

Drives a **real** WAGO 750-354 EtherCAT coupler from a Linux host (a
Raspberry Pi is the canonical target) via
`taktora-connector-ethercat`'s `EthercrabBusDriver`. Reads the
**750-430**'s 8 digital input bits and **mirrors** them straight to the
**750-530**'s 8 digital output bits every 10 ms scan cycle, printing on
every change.

This is the WAGO sibling of
[`ethercat-real-bus`](../ethercat-real-bus) (Beckhoff EK1100 + EL
terminals). CI compiles it but does NOT run it (no NIC, no WAGO coupler
in CI).

## Topology — why this differs from the Beckhoff example

The decisive difference is the EtherCAT topology:

- In the **Beckhoff** EK1100 example, *each* EL terminal is its own
  EtherCAT SubDevice with its own configured station address
  (EK1100 = `0x1000`, EL1008 = `0x1001`, …).
- Here, the **WAGO 750-354 coupler is the *only* EtherCAT SubDevice**
  (`0x1000`). The 750-430, 750-530, and 750-600 are **not** EtherCAT
  nodes — they are internal **K-bus** modules whose I/O is aggregated
  into the coupler's *single* process image. Both routings in this
  example therefore share `subdevice_address = 0x1000` and differ only
  by PDO direction.

### Process image map

| Module | Role | `PdoDirection` | SubDevice addr | Bit offset | Bits |
|---|---|---|---|---|---|
| 750-354 | EtherCAT coupler (the node) | — | `0x1000` | — | — |
| 750-430 | 8× DI | `Tx` (read) | `0x1000` | 0 | 8 |
| 750-530 | 8× DO | `Rx` (write) | `0x1000` | 0 | 8 |
| 750-600 | End module | — | — | — | — |

Inputs and outputs both sit at **bit offset 0** because they live in
*separate* Tx and Rx process images. The example assumes the 750-354
presents this all-digital configuration as a plain 1-input-byte /
1-output-byte process image. The `SUBDEV`, `DI_BIT_OFFSET`, and
`DO_BIT_OFFSET` constants in `src/main.rs` make a differing layout a
one-line edit — confirm yours with your EtherCAT config tool if the
mirror doesn't track.

## Hardware required

- WAGO **750-354/000-001** EtherCAT fieldbus coupler.
- WAGO **750-430** 8-channel 24 V digital input module, snapped onto
  the coupler's right side.
- WAGO **750-530** 8-channel 24 V digital output module (0.5 A),
  snapped on after the 750-430.
- WAGO **750-600** end module (bus terminator) on the far right.
- **24 V DC supply** on the coupler's system/field supply terminals.
- Standard Cat5e (or better) Ethernet cable from your host's wired NIC
  to the coupler's **IN** port.

## Host setup

The Pi's wired NIC must be UP but **without an IP** — EtherCAT is Layer
2 only, no TCP/IP. On Raspberry Pi OS:

```bash
# Identify the wired NIC. eth0 on Pi 4/5.
ip link show

# Bring it UP without an IP.
sudo ip link set eth0 up

# Stop dhcpcd / NetworkManager from fighting you. Easiest:
sudo nano /etc/dhcpcd.conf
#   denyinterfaces eth0
sudo systemctl restart dhcpcd
```

EtherCAT masters need raw socket access. Either run with `sudo`, or
grant the capability once (cleaner; survives reboots and doesn't put
cargo in root's home):

```bash
sudo setcap cap_net_raw,cap_net_admin=eip target/release/ethercat-wago-coupler
```

**Note:** `cargo build` replaces the binary, which clears the
capabilities. Re-run `setcap` after every rebuild.

## Build + run

```bash
cargo build --release
./target/release/ethercat-wago-coupler --nic eth0
```

Or cap the run length for a quick smoke test (each tick is the 10 ms
scan interval, so `--ticks 500` ≈ 5 seconds of runtime):

```bash
./target/release/ethercat-wago-coupler --nic eth0 --ticks 500
```

## What you should see

Bring-up, then one change-event per transition on the 750-430's 24 V
inputs — and the matching bit appears on the 750-530's outputs (its
channel LED lights) because the example mirrors input to output:

```
ethercat connector health at startup: Connecting
t=+   540ms  in=0b00000000 -> out=0b00000000  decimal=0
t=+  1180ms  ethercat health: Connecting -> Up
t=+  6020ms  in=0b00000001 -> out=0b00000001  decimal=1
t=+  6260ms  in=0b00000000 -> out=0b00000000  decimal=0
t=+  7710ms  in=0b00000010 -> out=0b00000010  decimal=2
t=+  7980ms  in=0b00000000 -> out=0b00000000  decimal=0
```

Bring-up usually completes within 1–2 seconds on a Pi 4 or 5. With no
inputs wired the 750-430 reports all zeros, the initial
`in=0b00000000 -> out=0b00000000` line prints, and nothing further
appears until you touch 24 V to one of the input channels.

## Troubleshooting

- **Stuck in `Connecting`, or transitions to `Down`.** Cable, power, or
  wrong NIC name. Run with `RUST_LOG=ethercrab=debug` for
  ethercrab-level diagnostics. Confirm 24 V on the coupler and that the
  cable is in the **IN** port.
- **`Permission denied (os error 13)` from ethercrab.** Raw socket caps
  not applied to the binary. Re-run `setcap`, or invoke with `sudo`.
- **Inputs read but outputs don't follow (or vice versa).** The process
  image offsets don't match your module order. The 750-430 inputs and
  750-530 outputs are placed in K-bus (left-to-right) order; if you
  added modules ahead of them, the bit offsets shift. Edit
  `DI_BIT_OFFSET` / `DO_BIT_OFFSET` in `src/main.rs`, and confirm the
  layout with your EtherCAT config tool.
- **Wrong SubDevice address.** If the coupler isn't the first device
  `ethercrab` scans, edit `SUBDEV` in `src/main.rs`.

## Debugging against in-tree changes

Same toggle as the other examples — flip the `[patch.crates-io]` block
in `Cargo.toml` to use local paths instead of crates.io:

```bash
scripts/examples-local.sh on        # uncomment the patch block
cd examples/ethercat-wago-coupler && cargo run --release -- --nic eth0
scripts/examples-local.sh off       # restore committed state
```

CI refuses to proceed if any example reports `on` — never commit while
the toggle is active.

## Hardware drill

The drill exercises the bus-level recovery path against real silicon.
Same three modes as `ethercat-real-bus`, adapted to the mirror rig.

### Rig

- Raspberry Pi 4 (or any Linux host with `CAP_NET_RAW`).
- WAGO 750-354 coupler, 750-430 (8 DI), 750-530 (8 DO), 750-600 end
  module, in that K-bus order.

### Procedure

1. **Normal mode smoke test.** Confirm bring-up, and that inputs mirror
   to outputs:
   ```bash
   sudo setcap cap_net_raw,cap_net_admin=eip target/release/ethercat-wago-coupler
   ./target/release/ethercat-wago-coupler --nic eth0 --mode normal --ticks 1000
   ```
   Pass criterion: `ethercat health: Connecting -> Up`; toggling a 24 V
   input on the 750-430 lights the matching 750-530 output LED and
   prints an `in=… -> out=…` line.

2. **Reconnect drill.** Run with a 60-second window. After ~10 s,
   physically unplug the coupler's input cable for at least 2 s, then
   replug. After ~30 s, briefly power-cycle the coupler.
   ```bash
   ./target/release/ethercat-wago-coupler --nic eth0 --mode drill --window 60
   ```
   Pass criterion: the drill summary reports `saw_degraded=true
   saw_recover_up=true`; the printed health transitions match
   `Up -> Degraded -> Connecting -> Up` for each event.

3. **Endurance run.** Run for 1 h.
   ```bash
   ./target/release/ethercat-wago-coupler --nic eth0 --mode endurance --duration 3600 2>&1 | tee wago-drill.log
   ```
   Pass criterion: `terminal_down=false`.

## What this shows

- `EthercrabBusDriver::new(&EXAMPLE_PDU_STORAGE, opts)` — real-bus driver
  construction (`bus-integration` feature).
- The `Connecting → Up` health handshake via
  `Connector::subscribe_health`.
- **Two PDO slices on a single SubDevice** — a Tx reader and an Rx
  writer sharing `subdevice_address = 0x1000`, the WAGO coupler's
  K-bus-aggregated process image. (Contrast the Beckhoff example, where
  each terminal is its own SubDevice.)
- A minimal `RawByteCodec` defined inline because `JsonCodec` can't
  decode the WAGO raw PDI byte. Purpose-built for this example and
  intentionally not promoted to `taktora-connector-codec`.
