//! Quality-managed process binary — demonstrates TSR_0009 cross-process
//! integrity isolation.
//!
//! This process:
//! - Runs an executor pinned to `IntegrityLevel::QualityManaged`
//! - Opens a READER on the `sc_to_qm` iceoryx2 channel (read capability)
//! - Receives and prints `CYCLE_COUNT` messages from the safety process
//! - Exits with code 0 after receiving all expected messages, code 1 on error
//!
//! The safety-critical process holds the WRITE capability and runs in a
//! separate OS process. Communication is exclusively via the iceoryx2
//! shared-memory channel — no shared mutable state (AOU_0008).

#![allow(clippy::doc_markdown)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iceoryx2::node::NodeBuilder;
use iceoryx2::prelude::ipc;
use taktora_executor::{ExecuteResult, Executor, ExecutorError, ItemFlow, item_with_triggers};
use taktora_connector_transport_iox::ServiceFactory;

use integrity_cross_process::{
    CYCLE_COUNT, CycleData, JsonCodec, MAX_PAYLOAD_BYTES, channel_descriptor,
};

fn main() {
    println!("[QM] Quality-managed process starting");

    // Create the iceoryx2 node for this process. Each process has its own
    // node instance, but they communicate over the same named services.
    let node = NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create iceoryx2 node");

    // Open the reader on the shared channel. The `open_or_create` semantics
    // in `ServiceFactory::create_reader` mean whichever process starts first
    // creates the service; the second opens the existing one.
    let factory = ServiceFactory::new(&node);
    let descriptor = channel_descriptor().expect("valid descriptor");
    let reader = factory
        .create_reader::<CycleData, _, _, MAX_PAYLOAD_BYTES>(&descriptor, JsonCodec)
        .expect("create reader");

    println!("[QM] Reader opened on channel '{}'", descriptor.name());

    // Build the executor pinned to QualityManaged. Any task whose
    // integrity_level() returns SafetyCritical will be rejected at add() time.
    let mut executor = Executor::builder()
        .integrity_level(taktora_executor::IntegrityLevel::QualityManaged)
        .worker_threads(0) // single-threaded for simplicity
        .build()
        .expect("build executor");

    println!("[QM] Executor built with IntegrityLevel::QualityManaged");

    // Shared counter of received messages, so the item can signal Stop when
    // it reaches CYCLE_COUNT.
    let received = Arc::new(AtomicU64::new(0));
    let received_clone = Arc::clone(&received);

    // Register a cyclic item that polls for incoming messages.
    executor
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                // Poll at ~200 Hz (5 ms interval) to ensure we drain the
                // subscriber queue faster than the publisher sends.
                d.interval(Duration::from_millis(5));
                Ok(())
            },
            move |ctx| -> ExecuteResult {
                // Drain all available messages this cycle (iceoryx2 may
                // deliver multiple samples per poll).
                while let Some(envelope) = reader.try_recv().expect("try_recv") {
                    let count = received_clone.fetch_add(1, Ordering::Relaxed) + 1;
                    println!(
                        "[QM] Received cycle {} @ timestamp {}",
                        envelope.value.cycle, envelope.value.timestamp_ns
                    );

                    if count >= CYCLE_COUNT {
                        println!("[QM] Received {} cycles, stopping", CYCLE_COUNT);
                        ctx.stop_executor();
                    }
                }
                Ok(ItemFlow::Continue)
            },
        ))
        .expect("add receiver item");

    // Run until the item signals Stop (after CYCLE_COUNT messages) or the
    // bounded timeout elapses. The timeout is a safety net: if the safety
    // process crashes or never starts, we don't hang indefinitely.
    let timeout = Duration::from_secs(30);
    println!("[QM] Running executor with {timeout:?} timeout");
    match executor.run_for(timeout) {
        Ok(()) => {
            // Executor stopped naturally (item signaled Stop).
            let final_count = received.load(Ordering::Relaxed);
            if final_count >= CYCLE_COUNT {
                println!("[QM] Quality-managed process exiting successfully");
                std::process::exit(0);
            } else {
                eprintln!(
                    "[QM] ERROR: Received only {}/{} cycles before stop",
                    final_count, CYCLE_COUNT
                );
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("[QM] ERROR: Executor run failed: {e:?}");
            std::process::exit(1);
        }
    }
}
