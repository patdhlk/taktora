//! Shared object-dictionary IR for fieldbus device descriptions.
//!
//! This crate holds the device-description concepts that `EtherCAT` (`CoE`) and
//! `CANopen` genuinely share: the Identity triple, the `CoE`/`CANopen` `DataType`
//! set, and the object-dictionary entry model. Transport-specific concepts
//! (PDO mapping, sync managers, mailbox) live in the per-fieldbus crates that
//! depend on this one.
//!
//! Types land in follow-on tasks; this is the crate scaffold.

mod data_type;
mod dictionary;
mod identity;

pub use data_type::DataType;
pub use dictionary::{Access, DictEntry};
pub use identity::Identity;
