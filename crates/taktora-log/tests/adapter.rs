//! Verifies the `LogSink` -> `log::Log` adapter forwards correctly.

use std::sync::{Arc, Mutex};
use taktora_log::{LogSink, LogSinkLogger};

#[derive(Default)]
struct Capture {
    records: Mutex<Vec<String>>,
}

impl LogSink for Capture {
    fn enabled(&self, _m: &log::Metadata<'_>) -> bool {
        true
    }
    fn emit(&self, record: &log::Record<'_>) {
        self.records
            .lock()
            .unwrap()
            .push(format!("{}|{}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

#[test]
fn adapter_forwards_to_sink() {
    let sink = Arc::new(Capture::default());
    let logger = LogSinkLogger::new(Arc::clone(&sink) as Arc<dyn LogSink>);

    let rec = log::Record::builder()
        .level(log::Level::Warn)
        .target("tk.adapter")
        .args(format_args!("hi"))
        .build();
    log::Log::log(&logger, &rec);

    let captured = sink.records.lock().unwrap().clone();
    assert_eq!(captured, vec!["WARN|hi".to_string()]);
}
