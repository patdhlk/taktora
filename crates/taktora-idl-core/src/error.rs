//! IR validation errors.

use thiserror::Error;

/// An error detected while [validating](crate::Module::validate) a module or
/// computing a [maximum serialized length](crate::Module::max_serialized_len).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IrError {
    /// A field, element, or service referenced a type name that the module
    /// does not define.
    #[error("`{referrer}` references undefined type `{name}`")]
    UnknownType {
        /// The struct/service that holds the dangling reference.
        referrer: String,
        /// The unresolved type name.
        name: String,
    },

    /// Two structs or enums share a name within one module.
    #[error("duplicate type definition `{name}`")]
    DuplicateType {
        /// The clashing name.
        name: String,
    },

    /// A struct transitively contains itself with no indirection, so its
    /// serialized length is unbounded. This is the one way the bounded-by-
    /// construction [`Type`](crate::Type) lattice can still diverge, and it is
    /// rejected here.
    #[error("recursive type: {} has no finite size", .cycle.join(" -> "))]
    RecursiveType {
        /// The reference cycle, from the entry struct back to itself.
        cycle: Vec<String>,
    },
}
