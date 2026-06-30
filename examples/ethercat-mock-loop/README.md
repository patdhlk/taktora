# ethercat-mock-loop

A 1 kHz control loop driven by the executor through
`taktora-connector-ethercat` with `MockBusDriver` configured in
loopback. Writes a `u16` counter to the outbound PDI slice and reads
the same value back from an inbound slice with a one-cycle lag.

This example requires `taktora-connector-ethercat 0.1.1` or later
(0.1.0 has a bug where the gateway's tokio runtime is built without
the timer driver enabled, causing the dispatcher to panic on its
first `tokio::time::sleep`).

## Run it

    cargo run

Or bound the duration:

    cargo run -- --ticks 200

On exit, the example prints a one-line summary, e.g.

    sent=1000 recv=404 max_lag=1

and exits non-zero if `max_lag > 2` or if `recv == 0` (the dispatcher
never moved a byte).

`recv` is materially lower than `sent` by design — the connector's
`CycleScheduler` follows a skip-not-catch-up policy, so when the
executor item is paced faster than the gateway's 1 ms cycle can pump,
the outbound bridge saturates and ~60% of payloads get dropped before
they reach the dispatcher. The example asserts the loopback **round-
trips correctly** (`max_lag <= 2`, `recv > 0`), not 1:1 throughput.
For lossless coupling on a real bus, pace the publisher slower than
the cycle time or use a wider routing slice with chunked payloads.

## What this shows

- `EthercatConnector::new(state, MockBusDriver, BinaryCodec)` — no
  `bus-integration` feature, no hardware.
- Paired bit-slice routings on the same SubDevice (`PdoDirection::Rx`
  for the writer, `PdoDirection::Tx` for the reader) at the same bit
  offset, since `MockBusDriver`'s loopback copies outputs to inputs at
  the same offset.
- A single interval-triggered `ExecutableItem` doing the standard
  "write outputs, read inputs, advance" control-loop pattern.
- A health-polling item that logs `ConnectorHealthKind` transitions.

## Payload encoding note

`pdi::write_routing` rejects payloads shorter than `bit_length / 8`
bytes, so the wire form needs a fixed width. This example uses
`BinaryCodec` (big-endian — the network / EtherCAT-PDI byte order) from
`taktora-connector-codec`, behind its opt-in `binary` cargo feature.
Fixed-width primitives encode to a constant length, so a `u16` is
always exactly 2 bytes regardless of value: the counter goes onto the
wire as a real integer and the routing's `bit_length` is a static 16.

## What to tweak

- `--ticks`, the interval (`Duration::from_millis(1)` → other rates).
- Bit offsets / lengths inside the two `EthercatRouting::new` calls
  for a different routing layout.
- Swap `MockBusDriver` for `EthercrabBusDriver` (and enable the
  `bus-integration` feature on `taktora-connector-ethercat`) once
  you have hardware and an `ETHERCAT_TEST_NIC` to point at.
