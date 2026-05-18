//! REQ_0812 + REQ_0814: producer doesn't block; reconnect drains the ring.

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use taktora_log_dlt::flusher::{FlusherConfig, spawn_flusher};
use taktora_log_dlt::level_table::LevelTable;
use taktora_log_dlt::ring::OfflineRing;
use taktora_log_dlt::transport::TransportConfig;

#[test]
fn producer_pushes_bytes_to_listening_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("dlt.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 5];
        conn.read_exact(&mut buf).unwrap();
        buf
    });

    let level_table = Arc::new(LevelTable::new(log::Level::Info));
    let ring = Arc::new(OfflineRing::with_capacity(64));
    let (handle, tx) = spawn_flusher(FlusherConfig {
        transport: TransportConfig::Uds(sock),
        ring: Arc::clone(&ring),
        level_table: Arc::clone(&level_table),
        reconnect_initial_backoff: Duration::from_millis(10),
        reconnect_max_backoff: Duration::from_millis(50),
    });

    let t0 = Instant::now();
    tx.send(b"hello".to_vec()).expect("non-blocking send");
    assert!(
        t0.elapsed() < Duration::from_millis(5),
        "send must not block"
    );

    let received = server.join().unwrap();
    assert_eq!(&received, b"hello");
    handle.shutdown();
}
