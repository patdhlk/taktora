//! REQ_0805: events emitted via tracing::* flow into the active LogSink.

use std::sync::{Arc, Mutex};

use taktora_log::{LogSink, init};

#[derive(Default)]
struct Capture(Mutex<Vec<String>>);

impl LogSink for Capture {
    fn enabled(&self, _m: &log::Metadata) -> bool {
        true
    }
    fn emit(&self, record: &log::Record) {
        self.0.lock().unwrap().push(format!(
            "{}|{}|{}",
            record.level(),
            record.target(),
            record.args()
        ));
    }
    fn flush(&self) {}
}

#[test]
fn tracing_event_is_captured_by_log_sink() {
    let sink: Arc<Capture> = Arc::new(Capture::default());
    init()
        .with_sink(Arc::clone(&sink) as Arc<dyn LogSink>)
        .with_max_level(log::LevelFilter::Trace)
        .start()
        .expect("first init");

    tracing::info!(target: "tk.bridge.test", "hello from tracing");

    let records = sink.0.lock().unwrap().clone();
    assert!(
        records
            .iter()
            .any(|r| r.contains("hello from tracing") && r.contains("tk.bridge.test")),
        "tracing event not captured by log sink: {records:?}"
    );
}
