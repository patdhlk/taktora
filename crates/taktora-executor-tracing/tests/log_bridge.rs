#![allow(missing_docs)]

#[test]
fn observer_events_flow_through_taktora_log_via_tracing_bridge() {
    use std::sync::{Arc, Mutex};
    use taktora_log::LogSink;

    #[derive(Default)]
    struct Capture(Mutex<Vec<String>>);
    impl LogSink for Capture {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn emit(&self, r: &log::Record) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{}|{}", r.target(), r.args()));
        }
        fn flush(&self) {}
    }

    let sink: Arc<Capture> = Arc::new(Capture::default());
    taktora_log::init()
        .with_sink(Arc::clone(&sink) as Arc<dyn LogSink>)
        .with_max_level(log::LevelFilter::Trace)
        .start()
        .expect("first init");

    let observer = taktora_executor_tracing::TracingObserver;
    taktora_executor::Observer::on_executor_up(&observer);

    let captured = sink.0.lock().unwrap().clone();
    assert!(
        captured
            .iter()
            .any(|r| r.contains("executor.up") && r.contains("taktora.executor")),
        "expected executor.up bridged via tracing-log: got {captured:?}"
    );
}
