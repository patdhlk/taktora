//! Safety-critical process binary — demonstrates TSR_0009 cross-process
//! integrity isolation.
//!
//! This process:
//! - Runs an executor pinned to `IntegrityLevel::SafetyCritical`
//! - Opens a WRITER on the `sc_to_qm` iceoryx2 channel (write capability)
//! - Publishes `CYCLE_COUNT` messages over that channel
//! - Exits with code 0 on success
//!
//! The quality-managed process holds the READ capability and runs in a
//! separate OS process. Communication is exclusively via the iceoryx2
//! shared-memory channel — no shared mutable state (AOU_0008).

#![allow(clippy::doc_markdown)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iceoryx2::node::NodeBuilder;
use iceoryx2::prelude::ipc;
use taktora_executor::{
    ExecutableItem, ExecuteResult, Executor, ExecutorError, ItemFlow, item_with_triggers,
};
use taktora_connector_transport_iox::ServiceFactory;
use integrity_cross_process::{
    CYCLE_COUNT, CycleData, JsonCodec, MAX_PAYLOAD_BYTES, channel_descriptor, now_ns,
};

/// Wrapper that marks an item as safety-critical.
struct SafetyCriticalItem<I>(I);

impl<I: ExecutableItem> ExecutableItem for SafetyCriticalItem<I> {
    fn declare_triggers(&mut self, d: &mut taktora_executor::TriggerDeclarer<'_>) -> Result<(), ExecutorError> {
        self.0.declare_triggers(d)
    }

    fn execute(&mut self, ctx: &mut taktora_executor::Context<'_>) -> ExecuteResult {
        self.0.execute(ctx)
    }

    fn integrity_level(&self) -> taktora_executor::IntegrityLevel {
        taktora_executor::IntegrityLevel::SafetyCritical
    }
}

fn main() {
    println!("[SC] Safety-critical process starting");

    // Create the iceoryx2 node for this process. Each process has its own
    // node instance, but they communicate over the same named services.
    let node = NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create iceoryx2 node");

    // Open the writer on the shared channel. The `open_or_create` semantics
    // in `ServiceFactory::create_writer` mean whichever process starts first
    // creates the service; the second opens the existing one.
    let factory = ServiceFactory::new(&node);
    let descriptor = channel_descriptor().expect("valid descriptor");
    let writer = factory
        .create_writer::<CycleData, _, _, MAX_PAYLOAD_BYTES>(&descriptor, JsonCodec)
        .expect("create writer");

    println!("[SC] Writer opened on channel '{}'", descriptor.name());

    // Build the executor pinned to SafetyCritical. Any task whose
    // integrity_level() returns QualityManaged will be rejected at add() time.
    let mut executor = Executor::builder()
        .integrity_level(taktora_executor::IntegrityLevel::SafetyCritical)
        .worker_threads(0) // single-threaded for simplicity
        .build()
        .expect("build executor");

    println!("[SC] Executor built with IntegrityLevel::SafetyCritical");

    // Register a cyclic item that publishes one message per cycle.
    let cycle_count = Arc::new(AtomicU64::new(0));
    let cycle_count_clone = Arc::clone(&cycle_count);
    executor
        .add(SafetyCriticalItem(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                // Run at ~100 Hz (10 ms interval). Fast enough to complete
                // 100 cycles quickly, slow enough to avoid saturating the
                // transport during debugging.
                d.interval(Duration::from_millis(10));
                Ok(())
            },
            move |ctx| -> ExecuteResult {
                let current_cycle = cycle_count_clone.fetch_add(1, Ordering::Relaxed);

                let msg = CycleData {
                    cycle: current_cycle,
                    timestamp_ns: now_ns(),
                };

                writer.send(&msg).expect("send message");
                println!("[SC] Sent cycle {}/{}", current_cycle + 1, CYCLE_COUNT);

                if current_cycle + 1 >= CYCLE_COUNT {
                    println!("[SC] Published {} cycles, stopping", CYCLE_COUNT);
                    ctx.stop_executor();
                }
                Ok(ItemFlow::Continue)
            },
        )))
        .expect("add publisher item");

    // Run until the item signals Stop (after CYCLE_COUNT cycles).
    executor.run().expect("run executor");

    println!("[SC] Safety-critical process exiting");
}
