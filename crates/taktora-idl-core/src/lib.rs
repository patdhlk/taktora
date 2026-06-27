//! Shared bounded message-type IR for interface descriptions.
//!
//! This crate is the **message-plane** twin of [`taktora-fieldbus-od-core`].
//! Where `od-core` describes a *device* (identity + object dictionary + cyclic
//! process image), this crate describes a *message*: the structs, enums,
//! bounded sequences, and request/reply services that cross a channel. DBC,
//! OMG IDL, and ROS 2 `.msg`/`.srv` frontends all lower onto these types.
//!
//! # The boundedness invariant
//!
//! taktora runs `no_std`, no-heap, with fixed-layout `ConnectorEnvelope`
//! buffers sized at compile time ([`ChannelDescriptor<R, N>`] carries `N` as a
//! const generic). A message IR that admits an unbounded `string` or
//! `sequence<T>` cannot honour that. So this IR makes unboundedness
//! *unrepresentable*: [`Type::String`] and [`Type::Sequence`] always carry a
//! capacity. A frontend that meets an unbounded source type must supply a
//! bound (from an annotation) or reject the type at import — it cannot smuggle
//! it into the IR.
//!
//! Because every type is bounded by construction, every type has a finite
//! [maximum serialized length](Module::max_serialized_len). That bound is the
//! through-line to the runtime: it is what sizes the `const N` envelope buffer
//! a generated `WireType` writes into. The only way to make it diverge is a
//! recursive struct (a struct that transitively contains itself with no
//! indirection); [`Module::validate`] rejects exactly that.
//!
//! # What lives here vs. in a frontend
//!
//! This IR is *plane-generic and policy-free*. It carries source names
//! verbatim — identifier sanitisation is a codegen concern, centralised there
//! the way `od-core`'s consumers centralise it in `naming.rs`. Wire-layout
//! details that are specific to one description language (DBC signal
//! bit-packing, CDR alignment) do **not** live here; they ride alongside the
//! IR in the frontend crate that produced them.
//!
//! [`taktora-fieldbus-od-core`]: https://docs.rs/taktora-fieldbus-od-core
//! [`ChannelDescriptor<R, N>`]: # "taktora-connector-core::ChannelDescriptor"

mod error;
mod message;
mod module;
mod scalar;
mod ty;

pub use error::IrError;
pub use message::{EnumDef, EnumVariant, Field, Service, Struct};
pub use module::Module;
pub use scalar::Scalar;
pub use ty::{LENGTH_PREFIX_BYTES, Type, TypeName};
