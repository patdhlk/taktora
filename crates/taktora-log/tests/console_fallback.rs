//! REQ_0816: Console fallback installed when no daemon and no other logger.
//!
//! We can't redirect stderr in a unit test (logs go to the test
//! harness's stderr). Instead we exercise the `Console` sink's
//! `emit` directly with a custom writer.

use std::io::Write;
use std::sync::{Arc, Mutex};
use taktora_log::LogSink;
use taktora_log::console::Console;

#[derive(Default, Clone)]
struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn console_emits_level_target_and_message() {
    let writer = CapturedWriter::default();
    let console = Console::with_writer(writer.clone(), log::LevelFilter::Trace);
    let args = format_args!("hello {}", 42);
    let record = log::Record::builder()
        .level(log::Level::Info)
        .target("tk.test")
        .args(args)
        .build();
    console.emit(&record);

    let captured = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
    assert!(captured.contains("INFO"), "missing level in {captured:?}");
    assert!(
        captured.contains("tk.test"),
        "missing target in {captured:?}"
    );
    assert!(
        captured.contains("hello 42"),
        "missing message in {captured:?}"
    );
}
