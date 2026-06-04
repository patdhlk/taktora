//! The drain thread turns ring records into NDJSON and reports a summary.
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use taktora_telemetry_export::{CycleRing, PodRecord, spawn};

/// A `Write` sink that captures bytes in a shared buffer (so the test can read
/// what the drain thread wrote after it joins).
#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);
impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn drains_all_records_to_ndjson() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = SharedBuf(buf.clone());

    let (producer, consumer) = CycleRing::with_capacity(64).split();
    let writer = spawn(consumer, sink);

    for i in 0..5 {
        producer.push(PodRecord::new_healthy(i, 0, i, 1_000, 1_001, 1, 0, 100));
    }
    drop(producer); // not required, but mirrors real shutdown ordering

    let summary = writer.finish().expect("drain ok");
    assert_eq!(summary.written, 5);
    assert_eq!(summary.lapped, 0);

    let text = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[0].contains("\"cycle_index\":0"));
    assert!(lines[4].contains("\"cycle_index\":4"));
}
