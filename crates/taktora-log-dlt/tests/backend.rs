//! End-to-end smoke: backend encodes a record and the mock daemon
//! receives valid DLT bytes.

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::Duration;

use taktora_log::LogSink;
use taktora_log_dlt::{
    DltBackend,
    ids::{AppId, CtxId},
};

#[test]
fn end_to_end_emit_round_trips_via_dlt_core_parse() {
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

    let backend = DltBackend::builder()
        .app(AppId::new("TKEX").unwrap())
        .default_context(CtxId::new("MAIN").unwrap())
        .ecu_id("ECU1")
        .uds(&sock)
        .ring_capacity(32)
        .build()
        .expect("builds");

    let args = format_args!("hello e2e");
    let rec = log::Record::builder()
        .level(log::Level::Info)
        .target("tk.test")
        .args(args)
        .build();
    backend.emit(&rec);

    thread::sleep(Duration::from_millis(50)); // let flusher drain
    let bytes = receive.join().unwrap();

    // Parse with corrected API. `ParsedMessage` lives in `dlt_core::parse`,
    // not `dlt_core::dlt` (the plan text had this wrong; existing tests
    // such as `encode_round_trip.rs` use the correct path).
    let (_, parsed) = dlt_core::parse::dlt_message(&bytes, None, true).unwrap();
    let msg = match parsed {
        dlt_core::parse::ParsedMessage::Item(m) => m,
        other => panic!("expected ParsedMessage::Item, got {other:?}"),
    };
    assert_eq!(msg.extended_header.unwrap().application_id, "TKEX");

    backend.shutdown();
}
