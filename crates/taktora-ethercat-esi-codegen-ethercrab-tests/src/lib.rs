//! End-to-end landing pad: generated EtherCAT device drivers + decode tests.
#[allow(
    missing_docs,
    non_camel_case_types,
    clippy::all,
    clippy::pedantic,
    clippy::nursery
)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/devices.rs"));
}
