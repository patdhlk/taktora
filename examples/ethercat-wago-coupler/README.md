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

The 750-354's PDO assignment is **fixed** (`PdoAssign=0` in its ESI —
not remappable via CoE), and in each direction a 4-byte
fieldbus-coupler (FC) status/control header precedes the K-bus module
data. With this node (750-430 + 750-530) the image is 6 bytes in each
direction:

| PDO | Direction | Content | Byte offset | Bits |
|---|---|---|---|---|
| `0x1AFF` | Tx (read) | FC status: K-Bus Cycle Overrun Flag, hold/clear acks, 16-bit Diagnostics Status Word | 0 | 32 |
| `0x1A00` | Tx (read) | **750-430 digital inputs** | 4 | 8 |
| `0x1B01` | Tx (read) | padding | 5 | 8 |
| `0x16FF` | Rx (write) | FC control: overrun-flag disable, hold/clear requests, 16-bit Diagnostics Control Word | 0 | 32 |
| `0x1601` | Rx (write) | **750-530 digital outputs** | 4 | 8 |
| `0x1701` | Rx (write) | padding | 5 | 8 |

The I/O bytes therefore sit at **bit offset 32** in both directions —
the FC header comes first, *then* the K-bus data. Both routings share
`subdevice_address = 0x1000` and differ only by PDO direction. If you
add modules ahead of the 430/530 in the K-bus order, the offsets
shift; confirm yours by reading the PDO assignment (`0x1C12` /
`0x1C13` and the mapped PDO entries) with your EtherCAT config tool.
The `SUBDEV`, `DI_BIT_OFFSET`, and `DO_BIT_OFFSET` constants in
`src/main.rs` make a differing layout a one-line edit.

### The address DIP switch does not affect this example

The 750-354 carries an 8-position DIP switch for setting an address
(ID 1–255). **You can leave it at any value — this example does not use
it.** EtherCAT has two distinct address concepts, and the DIP switch
drives the one we don't address by:

- **Configured station address** — assigned by the master *at startup,
  by bus position*. `ethercrab`'s `init_single_group` hands the first
  device `0x1000`, the next `0x1001`, and so on. This is what the
  example matches on (`SUBDEV = 0x1000`), via `configured_address()`.
- **Station alias** (a.k.a. Explicit Device ID / "second address") —
  this is what the DIP switch sets. It is stored in the coupler's
  EEPROM and read back via `ethercrab`'s `alias_address()`. The alias
  only matters for master features this example doesn't use (Hot
  Connect, explicit device identification).

So the coupler is `configured_address = 0x1000` whether the DIP switch
reads `0`, `1`, or `255`. The DIP value and `SUBDEV` are unrelated —
do **not** set `SUBDEV` to the DIP number.

## Hardware required

- WAGO **750-354** (or /000-001) EtherCAT fieldbus coupler.
- WAGO **750-602** (or 750-601/610/613) 24 V DC **potential supply
  module**, snapped on immediately after the coupler — see the field
  supply note below.
- WAGO **750-430** 8-channel 24 V digital input module, snapped on
  after the supply module.
- WAGO **750-530** 8-channel 24 V digital output module (0.5 A),
  snapped on after the 750-430.
- WAGO **750-600** end module (bus terminator) on the far right.
- **24 V DC supply** — wired to the coupler's system-supply terminals
  AND to the potential supply module's field terminals.
- Standard Cat5e (or better) Ethernet cable from your host's wired NIC
  to the coupler's **IN** port.

### The ECO coupler provides no field power

The 750-354 is an **ECO** coupler: its two CAGE CLAMP terminals are
the **system supply only**. It has no field-supply terminals and no
power jumper contacts of its own — without a potential supply module
the 750-430/750-530 field electronics are completely unpowered. The
symptom is purely electrical and invisible on the bus: bring-up
reaches OP and the process image reads/writes fine, but input channel
LEDs stay dark no matter what you wire, and output channel LEDs never
light even when commanded on. (The Beckhoff EK1100 doesn't have this
trap — it feeds its power contacts from its own Up terminals.)

Snap a 24 V DC potential supply module (750-602 is the basic, fuseless
one) between the coupler and the first I/O module and feed 24 V/0 V
into it; its power jumper contacts distribute field power rightward to
every module after it.

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
- **Stuck in `Connecting` forever while bus frames flow (equal TX/RX
  deltas on the NIC).** The 750-354 refuses SAFE-OP → OP until it sees
  cyclic output process data: its SM watchdog (its ESI declares
  `SafeopOpTimeout=100` ms) trips during a traffic-less wait and the
  AL error `0x001B` latches, blocking the transition. This needs
  `taktora-connector-ethercat` ≥ 0.2.5, whose bring-up exchanges
  process data while walking into OP and acknowledges latched AL
  errors (`REQ_0841`). Older connectors deadlock here — the Beckhoff
  example never showed this because the EK1100 grants OP without
  traffic.
- **`Permission denied (os error 13)` from ethercrab.** Raw socket caps
  not applied to the binary. Re-run `setcap`, or invoke with `sudo`.
  Remember `cargo build` replaces the binary and silently clears the
  capabilities.
- **`in`/`out` oscillate 0 ↔ 1 every scan cycle with nothing wired.**
  The offsets point at the FC status/control header instead of the
  K-bus data (see "Process image map"). The mirror then feeds the
  K-Bus Cycle Overrun Flag (status bit 0) back into the overrun-flag
  *disable* control bit — a feedback oscillator through the coupler's
  diagnostic plumbing. Set `DI_BIT_OFFSET` / `DO_BIT_OFFSET` to 32.
- **Bring-up fine, image reads/writes fine, but input LEDs stay dark
  and output LEDs never light.** No field power — the ECO coupler
  doesn't provide it. See "The ECO coupler provides no field power"
  above; you need a 24 V DC potential supply module (e.g. 750-602)
  ahead of the I/O modules.
- **Inputs read but outputs don't follow (or vice versa).** The process
  image offsets don't match your module order. The 750-430 inputs and
  750-530 outputs are placed in K-bus (left-to-right) order; if you
  added modules ahead of them, the bit offsets shift. Edit
  `DI_BIT_OFFSET` / `DO_BIT_OFFSET` in `src/main.rs`, and confirm the
  layout with your EtherCAT config tool.
- **Wrong SubDevice address.** `SUBDEV` is the *configured station
  address* `ethercrab` assigns by bus position (`0x1000` for the first
  device), **not** the coupler's address DIP switch (see "The address
  DIP switch does not affect this example" above). Only edit `SUBDEV`
  if the coupler isn't the first device `ethercrab` scans.

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
- WAGO 750-354 coupler, 750-602 (24 V field supply), 750-430 (8 DI),
  750-530 (8 DO), 750-600 end module, in that K-bus order.

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
