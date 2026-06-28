---
name: pi-bringup
description: >-
  Use when bringing up a real EtherCAT bus on a Raspberry Pi (or any Linux
  host) with the taktora ethercat connector — running ethercat-real-bus /
  ethercat-wago-coupler against real silicon, or debugging a Pi bring-up that
  hangs (e.g. "stuck in Connecting on the Pi", "real bus won't reach OP",
  "EtherCAT permission denied", "Pi build runs out of memory"). A runbook of
  steps + failure modes; points at the example READMEs as source of truth.
---

# pi-bringup

This skill is the **runbook companion** to the real-bus EtherCAT examples. The
example READMEs are the source of truth for hardware, topology, process-image
layout, and the exact CLI flags — read the one matching your rig first:

- Beckhoff EK1100 + EL terminals: `examples/ethercat-real-bus/README.md`
  (each terminal is its own SubDevice).
- WAGO 750-354 coupler: `examples/ethercat-wago-coupler/README.md`
  (single SubDevice, K-bus-aggregated process image).

The driver under both is `taktora-connector-ethercat`'s `EthercrabBusDriver`
(`bus-integration` feature). The runbook below applies to real-bus bring-up
generally; the per-rig specifics (station addresses, byte offsets, watchdog
register numbers, `SUBDEV`/`*_BIT_OFFSET` constants) live in those READMEs and
in each example's src/main.rs — do not memorize them here, they drift.

## Runbook

1. **Build with limited parallelism on the Pi.** A full-parallelism build OOMs
   on a Pi's limited RAM. Cap the job count:

   ```bash
   cargo build --release -j3
   ```

2. **Bring the NIC up with no IP.** EtherCAT is Layer 2 only — the wired NIC
   must be UP but carry no IP, and dhcpcd/NetworkManager must be told to leave
   it alone. The README's "Host setup" section has the exact `ip link` /
   dhcpcd steps.

3. **Grant raw-socket capability (or run under sudo).** The master needs
   `CAP_NET_RAW` (+ `cap_net_admin`). Grant it once with `setcap` per the
   README, or run with `sudo` — and note **sudo needs a password on the Pi**, so
   an unattended/SSH invocation will silently block on the prompt if you forget.

4. **Run the example** with the right `--nic` (the README shows `--nic eth0`
   plus `--ticks` / `--mode` for smoke tests and the hardware drill).

## Failure modes

- **Stuck in `Connecting` forever, no error printed.** The usual cause is a
  missing `CAP_NET_RAW` — without the capability the master cannot open the raw
  socket and the bus just hangs in `Connecting` with no diagnostic. Re-apply
  `setcap` (or use sudo) and remember **`cargo build` replaces the binary and
  silently clears the caps** — re-run `setcap` after every rebuild. (If it
  surfaces as `Permission denied (os error 13)` instead, same fix.)

- **`into_op` / SAFE-OP to OP watchdog deadlock.** Some couplers (the WAGO
  750-354 is the documented case) refuse the SAFE-OP to OP transition until they
  see cyclic *output* process data; their SM watchdog trips during the
  traffic-less wait and an AL error latches, deadlocking bring-up. The fix lives
  in the connector — it must exchange process data while walking into OP and
  acknowledge the latched AL error. This is a known driver issue; the WAGO
  README's troubleshooting section names the connector version that resolves it.
  A coupler (e.g. Beckhoff EK1100) that grants OP without traffic never shows it.

- **Bring-up reaches OP and the image reads/writes, but I/O LEDs stay dark.**
  Class of gotcha: **missing field power**. The WAGO 750-354 is an ECO coupler
  with no field-supply terminals — without a potential supply module (the
  750-602 is the basic one) snapped in ahead of the I/O modules, the I/O
  electronics are unpowered. Invisible on the bus; see the WAGO README's "ECO
  coupler provides no field power".

- **Inputs/outputs read the wrong bits, or oscillate with nothing wired.**
  Class of gotcha: **process data does not start at byte 0**. On the WAGO
  coupler a fieldbus-coupler status/control header precedes the K-bus module
  data, so the real I/O sits at a non-zero bit offset; pointing at offset 0
  feeds the diagnostic header instead. The exact offset and the
  `*_BIT_OFFSET` constants are in the WAGO README's process-image map and
  `examples/ethercat-wago-coupler/src/main.rs` — confirm yours with an EtherCAT
  config tool rather than hard-coding a number that shifts when modules are
  added.

## Debugging against in-tree connector changes

Both examples ship a `[patch.crates-io]` toggle to build against local crate
paths instead of the published versions (`scripts/examples-local.sh on` / `off`).
CI refuses to proceed while the toggle is `on` — never commit with it active.
The READMEs' "Debugging against in-tree changes" sections give the exact steps.
