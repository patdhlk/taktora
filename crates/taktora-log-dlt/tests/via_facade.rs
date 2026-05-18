//! End-to-end via the facade — REQ_0800 + REQ_0801 + REQ_0806.
//!
//! Uses a UDS-bound mock daemon and `DltBackendBuilder::uds`, so the
//! file is Unix-only. A future TCP variant would extend coverage to
//! Windows.

#![cfg(unix)]

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use taktora_log::{LogSink, init};
use taktora_log_dlt::{
    DltBackend,
    ids::{AppId, CtxId},
};

#[test]
fn caller_uses_facade_macros_bytes_land_in_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("dlt.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let receive = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 1024];
        let n = conn.read(&mut buf).unwrap();
        buf.truncate(n);
        buf
    });

    let backend = Arc::new(
        DltBackend::builder()
            .app(AppId::new("TKEX").unwrap())
            .default_context(CtxId::new("MAIN").unwrap())
            .ecu_id("ECU1")
            .uds(&sock)
            .ring_capacity(32)
            .build()
            .unwrap(),
    );

    init()
        .with_sink(Arc::clone(&backend) as Arc<dyn LogSink>)
        .with_max_level(log::LevelFilter::Trace)
        .start()
        .expect("first init");

    taktora_log::info!(target: "tk.endtoend", "hello via facade");

    thread::sleep(Duration::from_millis(50));
    let bytes = receive.join().unwrap();
    // Per T10 review: use the correct API path
    let (_, parsed) = dlt_core::parse::dlt_message(&bytes, None, true).unwrap();
    let msg = match parsed {
        dlt_core::parse::ParsedMessage::Item(m) => m,
        other => panic!("expected ParsedMessage::Item, got {other:?}"),
    };
    assert_eq!(msg.extended_header.unwrap().application_id, "TKEX");
}
