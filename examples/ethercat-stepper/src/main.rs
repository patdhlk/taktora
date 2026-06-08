//! Codegen de-risk stub. Confirms `taktora-ethercat-esi-build` produces a
//! compilable EL7047 driver from the trimmed ESI. Replaced by the real example.

// These modules are host-unit-tested now; `main()` starts consuming their
// public API in Task 8. Until then this bin crate sees them as dead code, so
// allow it at the `mod` boundary (the module sources stay suppression-free).
#[allow(dead_code)]
mod codec;
#[allow(dead_code)]
mod control;
#[allow(dead_code)]
mod el7047;

// The stub `main()` below calls `EsiDevice::input_len`/`output_len` on the
// generated device; the trait must be in scope. (Task 8 replaces main.rs.)
use taktora_ethercat_esi_rt::EsiDevice;

#[allow(
    missing_docs,
    non_camel_case_types,
    dead_code,
    clippy::all,
    clippy::pedantic,
    clippy::nursery
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/devices.rs"));
}

fn main() {
    // Force the generated EL7047 type to be instantiated so codegen output is
    // type-checked, not just generated.
    let dev = generated::EL7047::default();
    println!(
        "EL7047 input_len={} output_len={}",
        dev.input_len(),
        dev.output_len()
    );
}
