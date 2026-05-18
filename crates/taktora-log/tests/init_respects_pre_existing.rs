//! REQ_0804: an already-installed `log::Log` is preserved.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use taktora_log::{InitError, LogSink, init};

static PRE_EXISTING_USED: AtomicBool = AtomicBool::new(false);

struct PreExisting;
impl log::Log for PreExisting {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, _: &log::Record) {
        PRE_EXISTING_USED.store(true, Ordering::SeqCst);
    }
    fn flush(&self) {}
}

struct NeverCalled;
impl LogSink for NeverCalled {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn emit(&self, _: &log::Record) {
        panic!("must not be called — pre-existing log::Log wins");
    }
    fn flush(&self) {}
}

#[test]
fn pre_existing_logger_not_overridden() {
    log::set_boxed_logger(Box::new(PreExisting)).expect("test owns the global logger");
    log::set_max_level(log::LevelFilter::Info);

    let result = init()
        .with_sink(Arc::new(NeverCalled) as Arc<dyn LogSink>)
        .start();
    assert!(matches!(result, Err(InitError::PreExistingLogger)));

    log::info!("ping");
    assert!(PRE_EXISTING_USED.load(Ordering::SeqCst));
}
