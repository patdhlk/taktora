//! REQ_0812 + REQ_0814: producer doesn't block; reconnect drains the ring.
//!
//! Both tests rely on `UnixListener` to model a co-located `dlt-daemon`;
//! they are Unix-only. A future TCP-based variant would extend coverage
//! to Windows.

#![cfg(unix)]

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
        summary_builder: None,
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

#[test]
fn mid_drain_write_failure_rebuffers_remainder() {
    use std::net::Shutdown;
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use taktora_log_dlt::flusher::{FlusherConfig, spawn_flusher};
    use taktora_log_dlt::level_table::LevelTable;
    use taktora_log_dlt::ring::OfflineRing;
    use taktora_log_dlt::transport::TransportConfig;

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("dlt.sock");

    // Pre-populate the offline ring with 5 records BEFORE the listener
    // exists. We use large records (256 KiB each) so a single write
    // dominates the kernel SNDBUF — once the peer half-closes, the
    // next write fails with EPIPE instead of being absorbed by the
    // kernel buffer. This is timing-dependent, not guaranteed: the
    // kernel may absorb one more write before the close propagates
    // (observed on macOS CI, issue #52), so the assertions below
    // tolerate at most one absorbed record.
    const REC_LEN: usize = 256 * 1024;
    let ring = Arc::new(OfflineRing::with_capacity(16));
    for i in 0..5u8 {
        // Tag each record with its index in the first byte so the
        // test can identify which survived re-buffering.
        let mut bytes = vec![i; REC_LEN];
        bytes[0] = b'0' + i;
        ring.push(bytes);
    }
    assert_eq!(ring.drops_since_last_drain(), 0);

    let level_table = Arc::new(LevelTable::new(log::Level::Info));

    // Start the listener BEFORE the flusher so the first reconnect
    // succeeds. We'll shutdown(RDWR) the connection after one read
    // to force the mid-drain failure on the next write.
    let listener = UnixListener::bind(&sock).unwrap();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; REC_LEN];
        std::io::Read::read_exact(&mut conn, &mut buf).unwrap();
        // Hard-shutdown both directions so one of the flusher's
        // next writes hits EPIPE (usually the very next one, but
        // the kernel buffer may absorb a single extra record).
        let _ = conn.shutdown(Shutdown::Both);
        drop(conn);
        drop(listener);
        buf[0]
    });

    let (handle, _tx) = spawn_flusher(FlusherConfig {
        transport: TransportConfig::Uds(sock.clone()),
        ring: Arc::clone(&ring),
        level_table: Arc::clone(&level_table),
        reconnect_initial_backoff: Duration::from_millis(10),
        reconnect_max_backoff: Duration::from_millis(100),
        summary_builder: None,
    });

    let first_tag = server.join().unwrap();
    assert_eq!(first_tag, b'0', "server should have received rec-0");

    // Give the flusher time to attempt the next write, fail, and
    // re-buffer the remaining 4 records.
    thread::sleep(Duration::from_millis(500));
    handle.shutdown();

    // Ring should still contain the records that were undrained when
    // the write failed: rec-1..rec-4, minus at most one record the
    // kernel accepted for the already-closed peer (a successful
    // `write` is not delivery — unavoidable loss at this layer, not
    // a flusher bug). With the pre-fix buggy implementation, only
    // ONE record would survive — the others would be silently
    // dropped from the local `drained` Vec — so `len >= 3` still
    // fully discriminates the regression.
    let remaining = ring.drain_all();
    let tags: Vec<u8> = remaining.iter().map(|b| b[0]).collect();
    assert!(
        remaining.len() >= 3,
        "expected at least 3 re-buffered records (at most one absorbed \
         by SNDBUF after peer shutdown), got {} with tags {:?}",
        remaining.len(),
        tags
    );
    let full = [b'1', b'2', b'3', b'4'];
    assert_eq!(
        tags,
        full[full.len() - tags.len()..],
        "re-buffered records must be a FIFO-ordered suffix of rec-1..rec-4"
    );
}
