//! REQ_0807: UDS and TCP transports both connect+write+read.
//! UDS test is Unix-only; TCP test runs on every supported platform.

#[cfg(unix)]
#[test]
fn uds_connect_and_write() {
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    use std::thread;

    use taktora_log_dlt::transport::{Transport, TransportConfig};

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("dlt-test.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 7];
        conn.read_exact(&mut buf).unwrap();
        buf
    });

    let mut t = Transport::connect(&TransportConfig::Uds(sock)).expect("uds connects");
    t.write_all(b"hello\r\n").expect("uds writes");
    drop(t);

    let received = server.join().unwrap();
    assert_eq!(&received, b"hello\r\n");
}

#[test]
fn tcp_connect_and_write() {
    use std::io::Read;
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::thread;
    use std::time::Duration;

    use taktora_log_dlt::transport::{Transport, TransportConfig};

    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 5];
        conn.read_exact(&mut buf).unwrap();
        buf
    });

    // Give the listener a moment to be ready (best-effort).
    thread::sleep(Duration::from_millis(20));

    let mut t = Transport::connect(&TransportConfig::Tcp(format!("127.0.0.1:{port}")))
        .expect("tcp connects");
    t.write_all(b"world").expect("tcp writes");
    drop(t);

    let received = server.join().unwrap();
    assert_eq!(&received, b"world");
}
