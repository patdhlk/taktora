//! REQ_0815: drop-oldest emits one summary record on reconnect drain.
//!
//! Uses a UDS-bound mock daemon, so the file is Unix-only. A future
//! TCP variant would extend coverage to Windows.

#![cfg(unix)]

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use taktora_log_dlt::{
    DltBackend,
    ids::{AppId, CtxId},
};

#[test]
fn overflow_then_reconnect_emits_summary() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("dlt.sock");

    // Backend started before the listener exists — the flusher's
    // reconnect-with-backoff loop sits failing until the test brings
    // a listener up. While disconnected we can populate the ring
    // directly to model what `emit` would do once the producer
    // queue is also full (REQ_0814 overflow path).
    let backend = Arc::new(
        DltBackend::builder()
            .app(AppId::new("TKEX").unwrap())
            .default_context(CtxId::new("MAIN").unwrap())
            .ecu_id("ECU1")
            .uds(&sock)
            .ring_capacity(2)
            .build()
            .unwrap(),
    );

    // Push 5 pre-encoded records into the ring (capacity 2).
    // After the 5th push, 3 will have been dropped.
    for i in 0..5u32 {
        let body = format!("payload {i}");
        let args = format_args!("{body}");
        let rec = log::Record::builder()
            .level(log::Level::Info)
            .target("tk.test")
            .args(args)
            .build();
        let bytes = backend.encoder().encode(&rec, i);
        backend.ring().push(bytes);
    }

    assert_eq!(
        backend.ring().drops_since_last_drain(),
        3,
        "ring of capacity 2 hit by 5 pushes should report 3 drops",
    );

    // Now bring up the listener — the flusher will reconnect, see
    // drops > 0, and emit one taktora.log.dropped summary record
    // before draining the remaining two ring entries.
    let listener = UnixListener::bind(&sock).unwrap();
    let collected = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 16384];
        thread::sleep(Duration::from_millis(200));
        let n = conn.read(&mut buf).unwrap();
        buf.truncate(n);
        buf
    });

    thread::sleep(Duration::from_millis(400));
    let bytes = collected.join().unwrap();

    // Parse every DLT message from the stream.
    let mut cursor = 0usize;
    let mut messages: Vec<dlt_core::dlt::Message> = Vec::new();
    while cursor < bytes.len() {
        match dlt_core::parse::dlt_message(&bytes[cursor..], None, true) {
            Ok((remaining, dlt_core::parse::ParsedMessage::Item(msg))) => {
                let consumed = bytes.len() - cursor - remaining.len();
                cursor += consumed;
                messages.push(msg);
            }
            _ => break,
        }
    }
    assert!(!messages.is_empty(), "no messages parsed from {bytes:?}");

    let summary_msg_body = messages.iter().find_map(|m| {
        if let dlt_core::dlt::PayloadContent::Verbose(args) = &m.payload {
            if let Some(arg0) = args.first() {
                if let dlt_core::dlt::Value::StringVal(s) = &arg0.value {
                    if s.starts_with("taktora.log.dropped") {
                        return Some(s.clone());
                    }
                }
            }
        }
        None
    });
    assert!(
        summary_msg_body.is_some(),
        "no taktora.log.dropped summary in {:?}",
        messages
            .iter()
            .map(|m| {
                if let dlt_core::dlt::PayloadContent::Verbose(args) = &m.payload {
                    if let Some(arg0) = args.first() {
                        if let dlt_core::dlt::Value::StringVal(s) = &arg0.value {
                            return s.clone();
                        }
                    }
                }
                String::new()
            })
            .collect::<Vec<_>>()
    );

    // Sanity: the summary should mention the count.
    let body = summary_msg_body.unwrap();
    assert!(
        body.contains("count=3"),
        "summary should report 3 dropped records, got: {body}"
    );

    backend.shutdown();
}
