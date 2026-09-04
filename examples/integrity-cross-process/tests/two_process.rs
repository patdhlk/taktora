//! Integration test for the cross-process integrity isolation example.
//! TEST_0197 — verifies TSR_0009 by spawning the safety-critical and
//! quality-managed processes as actual OS child processes and confirming
//! they communicate over iceoryx2 and exit successfully.

#![allow(clippy::doc_markdown, clippy::field_reassign_with_default)]

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Integration test: spawn both binaries, verify they complete successfully.
///
/// This test exercises the actual cross-process path:
/// - `quality_process` starts first (subscriber attaches before first publish)
/// - `safety_process` starts second (begins publishing)
/// - Both processes should exit with code 0 after 100 cycles
///
/// The test waits up to 60 seconds per process (generous timeout) and
/// kills hung children to prevent indefinite hangs.
#[test]
fn safety_and_quality_processes_communicate_over_iceoryx2() {
    // Locate the binaries built by `cargo build`. For integration tests of
    // this crate's own binaries, cargo sets `CARGO_BIN_EXE_<name>` env vars
    // pointing to the built executable.
    let quality_bin = env!("CARGO_BIN_EXE_quality_process");
    let safety_bin = env!("CARGO_BIN_EXE_safety_process");

    println!("[TEST] Quality binary: {quality_bin}");
    println!("[TEST] Safety binary:  {safety_bin}");

    // Flag to coordinate early exit if one child fails.
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_qm = Arc::clone(&stop_flag);
    let stop_flag_sc = Arc::clone(&stop_flag);

    // Spawn the quality-managed process first so the subscriber is attached
    // before the safety process publishes its first message.
    println!("[TEST] Spawning quality_process...");
    let mut qm_child = Command::new(quality_bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn quality_process");
    let qm_pid = qm_child.id();
    println!("[TEST] quality_process spawned with PID {qm_pid}");

    // Give the quality process a moment to open its reader and attach the
    // subscriber before the safety process starts publishing.
    thread::sleep(Duration::from_millis(500));

    // Spawn the safety-critical process.
    println!("[TEST] Spawning safety_process...");
    let mut sc_child = Command::new(safety_bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn safety_process");
    let sc_pid = sc_child.id();
    println!("[TEST] safety_process spawned with PID {sc_pid}");

    // Wait for both children with a timeout. We spawn two threads to wait
    // concurrently so one hung child doesn't block the other's reporting.
    let qm_handle = thread::spawn(move || {
        let timeout = Duration::from_secs(60);
        let start = std::time::Instant::now();
        loop {
            match qm_child.try_wait() {
                Ok(Some(status)) => {
                    println!("[TEST] quality_process exited with {status}");
                    return status;
                }
                Ok(None) => {
                    // Still running
                    if stop_flag_qm.load(Ordering::Relaxed) {
                        // Other child failed; kill this one
                        let _ = qm_child.kill();
                        panic!("quality_process killed because sibling failed");
                    }
                    if start.elapsed() > timeout {
                        let _ = qm_child.kill();
                        panic!("quality_process timed out after {timeout:?}");
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = qm_child.kill();
                    panic!("quality_process try_wait error: {e}");
                }
            }
        }
    });

    let sc_handle = thread::spawn(move || {
        let timeout = Duration::from_secs(60);
        let start = std::time::Instant::now();
        loop {
            match sc_child.try_wait() {
                Ok(Some(status)) => {
                    println!("[TEST] safety_process exited with {status}");
                    return status;
                }
                Ok(None) => {
                    // Still running
                    if stop_flag_sc.load(Ordering::Relaxed) {
                        // Other child failed; kill this one
                        let _ = sc_child.kill();
                        panic!("safety_process killed because sibling failed");
                    }
                    if start.elapsed() > timeout {
                        let _ = sc_child.kill();
                        panic!("safety_process timed out after {timeout:?}");
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = sc_child.kill();
                    panic!("safety_process try_wait error: {e}");
                }
            }
        }
    });

    // Join both wait threads and assert success.
    let qm_status = qm_handle.join().expect("qm wait thread panicked");
    let sc_status = sc_handle.join().expect("sc wait thread panicked");

    assert!(
        qm_status.success(),
        "quality_process exited with non-zero status: {qm_status}"
    );
    assert!(
        sc_status.success(),
        "safety_process exited with non-zero status: {sc_status}"
    );

    println!("[TEST] Both processes completed successfully");
}
