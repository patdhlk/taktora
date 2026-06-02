#![no_std]
#![warn(missing_docs)]
//! See plan docs/superpowers/plans/2026-06-02-motion-nc-spine.md

pub mod state;
pub use state::{Cia402State, controlword, decode_state};

pub mod power;
pub use power::{PowerStateMachine, PowerTarget};

pub mod drive;
pub use drive::Cia402Drive;
