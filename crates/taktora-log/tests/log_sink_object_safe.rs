//! REQ_0802: `LogSink` trait defines backend extension surface.
//!
//! This test pins the trait as object-safe — backends must be usable
//! as `Box<dyn LogSink>` and `Arc<dyn LogSink>` for runtime selection.

use taktora_log::LogSink;

#[test]
fn log_sink_is_object_safe() {
    fn assert_object_safe(_: &dyn LogSink) {}
    // Compile-time check — if LogSink loses object safety this won't compile.
    let _ = assert_object_safe;
}
