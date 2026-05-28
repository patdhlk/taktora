# ethercat-real-bus

Drives a **real** Beckhoff EK1100 + EL1008 from a Linux host (a
Raspberry Pi is the canonical target) via
`taktora-connector-ethercat`'s `EthercrabBusDriver`. Reads the EL1008's
8 digital input bits at a 10 ms cadence and prints the byte every
time it changes.

This is the real-hardware sibling of
[`ethercat-mock-loop`](../ethercat-mock-loop). CI compiles it but
does NOT run it (no NIC, no EK1100 in CI).

## Hardware required

- Beckhoff **EK1100** bus coupler.
- Beckhoff **EL1008** 8-channel 24V digital input terminal, snapped
  onto the EK1100's right side.
- Bus end cap (EL9011 ships with the EK1100).
- **24V DC supply** on the EK1100's `Us` and `Up` terminals.
- Standard Cat5e (or better) Ethernet cable from your host's wired
  NIC to the EK1100's **IN (X1)** port. The OUT (X2) port is for
  daisy-chaining additional couplers; leave it empty.

> **Topology assumption.** The example pins the EL1008 to the EtherCAT
> configured station address `0x1001`. `ethercrab` auto-assigns these
> starting at `0x1000` in bus-scan order, so EK1100 = `0x1000`,
> EL1008 = `0x1001`, EL2004 = `0x1002`, etc. The driver matches on
> the configured station address (not a 0-based topology index), so
> this needs to be the real EtherCAT address. If you have additional
> terminals between the EK1100 and EL1008, edit the `SUBDEV` constant
> in `src/main.rs` accordingly.

## Host setup

The Pi's wired NIC must be UP but **without an IP** — EtherCAT is
Layer 2 only, no TCP/IP. On Raspberry Pi OS:

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
sudo setcap cap_net_raw,cap_net_admin=eip target/release/ethercat-real-bus
```

**Note:** `cargo build` replaces the binary, which clears the
capabilities. Re-run `setcap` after every rebuild, or wrap it in a
small shell helper.

## Build + run

```bash
cargo build --release
./target/release/ethercat-real-bus --nic eth0
```

Or cap the run length for a quick smoke test (each tick is the
10 ms scan interval, so `--ticks 500` ≈ 5 seconds of runtime):

```bash
./target/release/ethercat-real-bus --nic eth0 --ticks 500
```

## What you should see

Bring-up, then (if any inputs are wired) one change-event per
transition on the EL1008's 24 V inputs. The excerpt below is from a
Pi 5 walking a 24 V probe across the eight input channels in order —
captured on the documented topology plus an extra EL2004 between the
EL1008 and the end cap:

```
0 [W] "Config::global_config()"
 | No config file was loaded, a config with default values will be used.
ethercat connector health at startup: Connecting
t=+   531ms  bits=0b00000000  decimal=0
ethercat connector health: Connecting -> Up
ethercat connector health: Up -> Degraded
t=+ 23510ms  bits=0b00000001  decimal=1
t=+ 23650ms  bits=0b00000000  decimal=0
t=+ 25360ms  bits=0b00000010  decimal=2
t=+ 25780ms  bits=0b00000000  decimal=0
t=+ 27170ms  bits=0b00000100  decimal=4
t=+ 27460ms  bits=0b00000000  decimal=0
t=+ 28580ms  bits=0b00001000  decimal=8
t=+ 28840ms  bits=0b00000000  decimal=0
t=+ 30190ms  bits=0b00010000  decimal=16
t=+ 30410ms  bits=0b00000000  decimal=0
t=+ 31370ms  bits=0b00100000  decimal=32
t=+ 31610ms  bits=0b00000000  decimal=0
t=+ 33730ms  bits=0b01000000  decimal=64
t=+ 33960ms  bits=0b00000000  decimal=0
t=+ 34730ms  bits=0b10000000  decimal=128
t=+ 34881ms  bits=0b00000000  decimal=0
```

Bring-up usually completes within 1–2 seconds on a Pi 4 or 5. With
no inputs wired the EL1008 just reports all zeros — bring-up still
runs to completion, the initial `bits=0b00000000` line prints, and
nothing further appears until you touch 24 V to one of the input
channels.

The `Up -> Degraded` line above is rig-specific: adding an output
terminal such as the EL2004 without writing to it makes the working
counter come back lower than ethercrab expects, which trips the
connector's asymmetric-PDO degradation flag. Reads from the EL1008
keep flowing. With the bare EK1100 + EL1008 topology this README
documents you stay at `Up` and never see the `Degraded` transition.

## Troubleshooting

- **Stuck in `Connecting`, or transitions to `Down`.** Cable, power,
  or wrong NIC name. Run with `RUST_LOG=ethercrab=debug` for
  ethercrab-level diagnostics. Confirm 24V on the EK1100 (the
  green `Us` LED should be lit) and that the cable is plugged into
  the **IN** port.
- **`Permission denied (os error 13)` from ethercrab.** Raw socket
  caps not applied to the binary. Re-run `setcap`, or invoke with
  `sudo`.
- **No bit changes shown.** Either nothing is wired to the EL1008's
  inputs, or the SubDevice address is wrong for your topology. Edit
  `SUBDEV` in `src/main.rs`.

## Debugging against in-tree changes

Same toggle as the other examples — flip the `[patch.crates-io]`
block in `Cargo.toml` to use local paths instead of crates.io:

```bash
scripts/examples-local.sh on        # uncomment the patch block
cd examples/ethercat-real-bus && cargo run --release -- --nic eth0
scripts/examples-local.sh off       # restore committed state
```

CI refuses to proceed if any example reports `on` — never commit
while the toggle is active.

## Hardware drill (TEST_0227)

The drill exercises the bus-level recovery path against real silicon.

### Rig

- Raspberry Pi 4 (or any Linux host with `CAP_NET_RAW`).
- Beckhoff EK1100 bus coupler.
- Beckhoff EL1008 (8 digital inputs) immediately right of the EK1100.
- Beckhoff EL2004 (4 digital outputs) immediately right of the EL1008.

### Procedure

1. **Normal mode smoke test.** Confirm bring-up, inputs read, outputs
   toggle:
   ```bash
   sudo setcap cap_net_raw=eip target/release/ethercat-real-bus
   ./target/release/ethercat-real-bus --nic eth0 --mode normal --ticks 1000
   ```
   Pass criterion: `ethercat health: Connecting -> Up`, EL2004 LEDs
   flicker on a 500 ms cadence, EL1008 bits read correctly.

2. **Reconnect drill.** Run with a 60-second window. After ~10 s,
   physically unplug the EK1100 input cable for at least 2 s, then
   replug. After ~30 s, briefly power-cycle the EK1100.
   ```bash
   ./target/release/ethercat-real-bus --nic eth0 --mode drill --window 60
   ```
   Pass criterion: the drill summary reports `saw_degraded=true
   saw_recover_up=true`; the printed health transitions match
   `Up -> Degraded -> Connecting -> Up` for each event.

3. **Endurance run.** Run for 1 h.
   ```bash
   ./target/release/ethercat-real-bus --nic eth0 --mode endurance --duration 3600 2>&1 | tee drill.log
   ```
   Pass criterion: `terminal_down=false`. Archive `drill.log` as
   `docs/superpowers/specs/2026-05-28-ethercrab-bus-driver-drill.log`
   (this file is gitignored — kept locally only).

## What this shows

- `EthercrabBusDriver::new(&PDU_STORAGE, opts)` — real-bus driver
  construction (`bus-integration` feature).
- The `Connecting → Up` health handshake via
  `Connector::subscribe_health`.
- `ChannelReader<u8, N>` over an `EthercatRouting` PDO slice — the
  same surface every other connector in this workspace exposes.
- A minimal `RawByteCodec` defined inline because `JsonCodec` can't
  decode the EL1008's raw PDI byte. The codec is purpose-built for
  this example and intentionally not promoted to the workspace's
  `taktora-connector-codec`.
