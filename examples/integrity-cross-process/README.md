# Cross-Process Integrity Isolation Example

Demonstrates **TSR_0009**: safety-critical and quality-managed tasks running in **distinct OS processes**, communicating exclusively over **iceoryx2 shared-memory channels**.

## What This Example Shows

- **Process isolation**: Two separate binaries (`safety_process` and `quality_process`) run as independent OS processes.
- **Integrity pinning**: Each process builds an executor pinned to a single `IntegrityLevel`:
  - `safety_process` → `IntegrityLevel::SafetyCritical`
  - `quality_process` → `IntegrityLevel::QualityManaged`
- **Per-process capability**: Each process holds a distinct capability on the shared channel:
  - `safety_process` → **WRITE** (publishes `CycleData` messages)
  - `quality_process` → **READ** (receives and prints messages)
- **Zero shared mutable state**: The two processes communicate **only** via the iceoryx2 `sc_to_qm` service (shared-memory pub/sub). No files, sockets, or shared memory outside the iceoryx2 channel (aligns with **AOU_0008**).
- **Real cross-process shared memory**: iceoryx2 `ipc::Service` uses true shared memory; both processes must be running on the same host and can see each other's publications in real time.

## Topology

```
┌─────────────────────────────┐         ┌──────────────────────────────┐
│   safety_process            │         │   quality_process            │
│   (SafetyCritical executor) │         │   (QualityManaged executor)  │
│                             │         │                              │
│   ┌─────────────────────┐   │         │   ┌──────────────────────┐   │
│   │ Cyclic publisher    │   │         │   │ Cyclic receiver      │   │
│   │ (100 cycles @ 10ms) │   │         │   │ (polls @ 5ms)        │   │
│   └──────────┬──────────┘   │         │   └─────────┬────────────┘   │
│              │              │         │             │                │
│              │ WRITE        │         │             │ READ           │
│              ▼              │         │             ▼                │
│   ┌─────────────────────┐   │         │   ┌──────────────────────┐   │
│   │ ChannelWriter       │   │         │   │ ChannelReader        │   │
│   └──────────┬──────────┘   │         │   └─────────┬────────────┘   │
└──────────────┼──────────────┘         └─────────────┼───────────────┘
               │                                       │
               │    iceoryx2 shared memory service     │
               │    "integrity_demo.sc_to_qm"          │
               └───────────────────┬───────────────────┘
                                   │
                          CycleData { cycle, timestamp_ns }
```

## Building

This example is a **standalone workspace** (not a member of the main taktora workspace). Build both binaries:

```bash
cd examples/integrity-cross-process
cargo build --release
```

The release binaries will be at:
- `target/release/safety_process`
- `target/release/quality_process`

## Running

### Option 1: Two Terminals (Recommended for First Run)

**Terminal 1** — start the quality-managed process first so it's ready to receive:
```bash
cargo run --release --bin quality_process
```

**Terminal 2** — start the safety-critical process to begin publishing:
```bash
cargo run --release --bin safety_process
```

You should see coordinated output:
- **Terminal 1**: `[QM] Received cycle 0 @ timestamp ...`, `[QM] Received cycle 1 @ timestamp ...`, etc.
- **Terminal 2**: `[SC] Sent cycle 1/100`, `[SC] Sent cycle 2/100`, etc.

Both processes will stop after 100 cycles (the safety process after sending 100, the quality process after receiving 100). Both should exit with code 0.

### Option 2: Single Terminal (Background the Reader)

```bash
cargo run --release --bin quality_process &
QM_PID=$!
cargo run --release --bin safety_process
wait $QM_PID
```

### Option 3: Integration Test (Automated)

The integration test spawns both processes as child processes, waits for them to complete, and verifies their exit codes:

```bash
cargo test --test two_process
```

The test uses `std::process::Command` to spawn the actual binaries, so it exercises the real cross-process communication path.

## iceoryx2 Service Lifecycle

- **Service creation**: The first process to call `ServiceFactory::create_writer` or `create_reader` creates the iceoryx2 service `integrity_demo.sc_to_qm` with the default QoS settings (configured in the transport-iox factory). The second process opens the existing service.
- **Service cleanup**: iceoryx2 services are persistent shared-memory resources. When both processes exit, the service may remain in `/dev/shm` (Linux) or equivalent. iceoryx2 automatically cleans up stale services on the next `open_or_create`, so you typically don't need to manually delete them. If you encounter `service already exists with incompatible QoS` errors, you can force cleanup by stopping all processes and deleting the service manually or restarting the iceoryx2 daemon (if running one — this example does not require the daemon).

## Verifying TSR_0009

This example verifies **TSR_0009** (cross-process hosting mode with per-process integrity pinning) by:

1. **Process boundary**: `ps` or the integration test shows two distinct process IDs.
2. **Integrity enforcement**: Each executor is pinned via `ExecutorBuilder::integrity_level(level)`. Try adding a task with the wrong level to see the executor reject it at `add()` time.
3. **IPC-only communication**: The only shared state is the iceoryx2 service — no files, pipes, sockets, or global variables.
4. **Per-process capability**: The safety process holds only a `ChannelWriter`; the quality process holds only a `ChannelReader`. Neither process can call the other's methods.

## Troubleshooting

### "Service already exists with incompatible QoS"

The iceoryx2 service was created by a previous run with different settings. Stop all processes and run:
```bash
# Linux / macOS
rm -f /dev/shm/iox2_*
```

Or restart the iceoryx2 daemon if you're running one (not required for this example).

### "No samples received" / quality process times out

- **Ordering**: Start the `quality_process` *before* the `safety_process` so the subscriber is attached when the first message is published. iceoryx2 publish/subscribe is not retroactive by default (though this example configures `history_size: 1` to allow late joiners to see the last published sample).
- **Timing**: The quality process has a 30-second timeout. If the safety process is delayed or crashes, the quality process will exit with code 1.
- **Shared memory permissions**: Ensure `/dev/shm` (or the platform equivalent) is writable. iceoryx2 requires shared-memory access.

### Integration test hangs

The test has a 60-second timeout per child process and kills hung children. If the test hangs, check:
- The binaries are built (`cargo build` before `cargo test`).
- No orphaned processes from a prior run (`pkill -f safety_process; pkill -f quality_process`).

## Related Requirements

- **TSR_0009**: Cross-process hosting mode with per-process integrity level enforcement.
- **AOU_0008**: No shared mutable state outside the iceoryx2 transport.
- **TSR_0003**: Integrity isolation primitives (each executor pinned to a single level).

## Next Steps

To adapt this example for your own multi-process topology:

1. Define your message types in the shared `lib.rs` (or a separate shared crate).
2. Declare the channel name(s) and QoS in static `ChannelSpec` constants.
3. Split your tasks across binaries by integrity level or functional role.
4. Coordinate startup: the reader should start first or use iceoryx2's history to tolerate late joins.
5. Add an integration test that spawns all binaries and verifies the end-to-end flow.
