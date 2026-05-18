//! REQ_0801: taktora-log re-exports log macros.
//!
//! Compile-only smoke test — if these macros aren't re-exported the
//! test binary won't compile.

use taktora_log::{debug, error, info, trace, warn};

#[test]
fn macros_compile_when_used_via_taktora_log() {
    info!("info from taktora_log::info!");
    warn!("warn");
    error!("error");
    debug!("debug");
    trace!("trace");
}
