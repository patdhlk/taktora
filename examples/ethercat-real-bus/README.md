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

```
ethercat connector health at startup: Connecting
ethercat connector health: Connecting -> Up
t=+    12ms  bits=0b00000000  decimal=0
t=+  1842ms  bits=0b00000001  decimal=1
t=+  2103ms  bits=0b00000011  decimal=3
...
```

Bring-up usually completes within 1–2 seconds on a Pi 4. If you see
only `Connecting -> Up` and then no further output, that's expected
with no inputs wired — the EL1008 just reports all zeros. The bus is
alive; press a button on any 24V input channel to watch the byte
change.

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
