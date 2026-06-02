#![no_std]
#![warn(missing_docs)]
//! Fieldbus-independent `CiA` 402 profile: statusword/controlword semantics,
//! the per-axis power state machine (`PowerStateMachine`), and the
//! `Cia402Drive` process-image accessor trait. `no_std`, no dependencies.

pub mod state;
pub use state::{Cia402State, controlword, decode_state};

pub mod power;
pub use power::{PowerStateMachine, PowerTarget};

pub mod drive;
pub use drive::Cia402Drive;
