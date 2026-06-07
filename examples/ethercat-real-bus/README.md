# ethercat-real-bus

Drives a **real** Beckhoff EK1100 + EL1008 + EL2004 from a Linux host
(a Raspberry Pi is the canonical target) via
`taktora-connector-ethercat`'s `EthercrabBusDriver`. Reads the EL1008's
8 digital input bits at a 10 ms cadence and prints them — both as the
raw byte and as the named channels decoded by an ESI-generated typed
driver — every time they change, and toggles the EL2004's first output
on a 500 ms cadence through the matching generated output driver.

## ESI-generated typed drivers

The example does **not** hand-decode PDI bits. At build time, `build.rs`
runs `taktora-ethercat-esi-build` over the trimmed Beckhoff ESI files in
[`esi/`](esi/) and generates `$OUT_DIR/devices.rs`, which `src/main.rs`
pulls in via `mod generated { include!(...); }`. The EL1008 input byte
is fed through `generated::EL1008::decode_inputs`, exposing named fields
(`dev.channel_1.input … channel_8`); the EL2004 output is built by
setting `el2004.channel_1.output` and calling
`generated::EL2004::encode_outputs`. The hand-written PDO routing widths
(8 / 4 bits) are cross-checked against the generated drivers'
byte-rounded `input_len()` / `output_len()` at startup.

Two **trimmed** real-device fixtures ship committed
(`esi/beckhoff_el1008.xml`, `esi/beckhoff_el2004.xml`) so the demo
builds standalone from a clean checkout and the Pi build stays fast. To
generate the whole Beckhoff catalog instead, drop the full vendor files
(e.g. `Beckhoff EL1xxx.xml`, `Beckhoff EL2xxx.xml`) into `esi/` — codegen
handles them, and `.gitignore` keeps those large files local-only.

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

Each change line now carries both the raw byte and the ESI-decoded
named channels:

```
0 [W] "Config::global_config()"
 | No config file was loaded, a config with default values will be used.
ethercat connector health at startup: Connecting
t=+   531ms  bits=0b00000000  decimal=0  ch1=0 ch2=0 ch3=0 ch4=0 ch5=0 ch6=0 ch7=0 ch8=0
ethercat connector health: Connecting -> Up
ethercat connector health: Up -> Degraded
t=+ 23510ms  bits=0b00000001  decimal=1  ch1=1 ch2=0 ch3=0 ch4=0 ch5=0 ch6=0 ch7=0 ch8=0
t=+ 23650ms  bits=0b00000000  decimal=0  ch1=0 ch2=0 ch3=0 ch4=0 ch5=0 ch6=0 ch7=0 ch8=0
t=+ 25360ms  bits=0b00000010  decimal=2  ch1=0 ch2=1 ch3=0 ch4=0 ch5=0 ch6=0 ch7=0 ch8=0
t=+ 28580ms  bits=0b00001000  decimal=8  ch1=0 ch2=0 ch3=0 ch4=1 ch5=0 ch6=0 ch7=0 ch8=0
t=+ 34730ms  bits=0b10000000  decimal=128 ch1=0 ch2=0 ch3=0 ch4=0 ch5=0 ch6=0 ch7=0 ch8=1
```

(`ch1` is bit 0, `ch8` is bit 7 — Lsb0 ordering, matching the EL1008's
Tx PDO. The EL2004's first output toggles on its own 500 ms cadence.)

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
- `PDO_MAP` declaring per-SubDevice `expected_wkc` (EL1008 = 2,
  EL2004 = 1; without it a dead bus reads as healthy) and, on the
  EL2004 output terminal, a 50 ms SM watchdog
  (`.with_sm_watchdog(SmWatchdog::from_timeout_us(50_000))`) —
  AOU_0016's FTTI/2 bound, programmed and read-back-verified by the
  driver at bring-up and recovery (REQ_0846). On a master stop the
  EL2004 drops its outputs to safe state within that window —
  observable in the reconnect drill as the channel LEDs going dark at
  unplug.
- A minimal `RawByteCodec` defined inline because `JsonCodec` can't
  decode the EL1008's raw PDI byte. The codec is purpose-built for
  this example and intentionally not promoted to the workspace's
  `taktora-connector-codec`.
- The ESI device-codegen spine end-to-end: `taktora-ethercat-esi-build`
  in `build.rs` turning `esi/*.xml` into typed `EsiDevice`
  implementations, layered on top of the raw byte channels —
  `EL1008::decode_inputs` for named input channels and
  `EL2004::encode_outputs` for the toggled output. The connector,
  routing, modes, health pump, and `RawByteCodec` are unchanged; the
  typed layer sits on top.
