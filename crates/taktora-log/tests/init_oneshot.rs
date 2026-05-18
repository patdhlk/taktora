//! REQ_0803 / REQ_0804: one-shot init; pre-existing logger wins.
//!
//! `log::set_logger` is global state. To test (a) the second init
//! returns the documented error and (b) a pre-existing logger is not
//! overridden, we run two subprocess-style sub-tests via assert_cmd
//! style would be heavyweight. Instead each sub-test runs in its own
//! integration-test binary by living in its own file under tests/.

use std::sync::Arc;

use taktora_log::{InitError, LogSink, console::Console, init};

#[test]
fn second_init_returns_already_initialized() {
    init()
        .with_sink(Arc::new(Console::stderr_default()) as Arc<dyn LogSink>)
        .start()
        .expect("first init must succeed");

    let second = init()
        .with_sink(Arc::new(Console::stderr_default()) as Arc<dyn LogSink>)
        .start();

    assert!(matches!(second, Err(InitError::AlreadyInitialized)));
}
