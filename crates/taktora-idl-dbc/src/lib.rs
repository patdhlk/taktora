//! DBC (CAN database) frontend for the message plane.
//!
//! DBC is the natural *first* frontend for [`taktora_idl_core`]: it is bounded
//! by construction. Every message has a fixed data length (`DLC`) and every
//! signal a fixed bit width, so the boundedness invariant the IR enforces is
//! never even at risk — lowering a `.dbc` cannot produce an unbounded type.
//! That makes it the cleanest possible proof of the description -> IR -> codegen
//! pipeline before harder frontends (OMG IDL, ROS 2) introduce real
//! unbounded-sequence rejection.
//!
//! # Pipeline
//!
//! ```text
//!   .dbc text  --[parse]-->  DbcDatabase  --[lower]-->  (idl_core::Module, DbcLayout)
//! ```
//!
//! * [`parse`] turns DBC text into a [`DbcDatabase`] — the faithful, DBC-shaped
//!   AST (messages, signals, value tables, nodes).
//! * [`lower`] projects that onto two outputs:
//!   * an [`idl_core::Module`](taktora_idl_core::Module) — the *plane-generic
//!     logical* view: each message becomes a struct, each signal a field, each
//!     value table an enum. This is what every backend shares.
//!   * a [`DbcLayout`] — the *DBC-specific physical* view: per-signal start
//!     bit, length, byte order, and the `factor`/`offset` scaling. This rides
//!     alongside the module and is consumed only by the CAN-frame `WireType`
//!     codegen, keeping bit-packing out of the shared IR.
//!
//! Source names are carried through verbatim; turning them into valid target
//! identifiers is a codegen-stage policy concern, not a frontend one.

mod ast;
mod layout;
mod lower;
mod parse;

pub use ast::{ByteOrder, DbcDatabase, DbcMessage, DbcSignal, DbcValueTable, Multiplexer};
pub use layout::{DbcLayout, FrameLayout, SignalLayout};
pub use lower::{LowerError, LoweredDbc, lower};
pub use parse::{ParseError, ParseErrorKind, parse};
