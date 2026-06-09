//! Library crate for the `ethercat-stepper` example.
//!
//! The example is shipped as a thin binary (`main.rs`) on top of this
//! library so that integration tests under `tests/` can import the
//! modules directly as `ethercat_stepper::...`.

/// ESI-generated typed device drivers. `build.rs` runs
/// `taktora-ethercat-esi-build` over `esi/*.xml` and writes
/// `$OUT_DIR/devices.rs`; this module `include!`s it. The `allow`s mirror
/// the codegen landing-pad crate — generated code is not held to this
/// crate's lint bar.
#[allow(
    missing_docs,
    non_camel_case_types,
    dead_code,
    clippy::all,
    clippy::pedantic,
    clippy::nursery
)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/devices.rs"));
}

/// netcfg-generated bus configuration. `build.rs` runs
/// `taktora-ethercat-netcfg-build` over `network.yaml` and writes
/// `$OUT_DIR/network.rs`; this module `include!`s it.
#[allow(
    missing_docs,
    non_camel_case_types,
    dead_code,
    clippy::all,
    clippy::pedantic,
    clippy::nursery
)]
pub mod generated_net {
    include!(concat!(env!("OUT_DIR"), "/network.rs"));
}

pub mod codec;
pub mod control;
pub mod el7047_adapter;
pub mod el7047_domain;
