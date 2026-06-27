//! The bounded type lattice.

use serde::{Deserialize, Serialize};

use crate::Scalar;

/// Length-prefix width assumed for the [`Type::String`] and [`Type::Sequence`]
/// *upper-bound* size estimate.
///
/// idl-core does not own the wire format — a backend's generated `WireType`
/// does. This constant only feeds [`Module::max_serialized_len`], which
/// produces the buffer-sizing upper bound. It matches the 4-byte length prefix
/// of CDR/XCDR (the DDS/ROS 2 encoding); a backend that frames sequences more
/// tightly will serialize *within* this bound, never beyond it.
///
/// [`Module::max_serialized_len`]: crate::Module::max_serialized_len
pub const LENGTH_PREFIX_BYTES: usize = 4;

/// A reference to a named [`Struct`](crate::Struct) or [`EnumDef`](crate::EnumDef),
/// resolved within the owning [`Module`](crate::Module).
///
/// Carried verbatim from the source description; sanitisation into a target
/// language identifier is a codegen concern, not an IR one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeName(pub String);

impl TypeName {
    /// Borrow the underlying name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for TypeName {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

/// A message field type.
///
/// Every variant is **bounded by construction**: there is no way to spell an
/// unbounded string or sequence. [`Type::String`] and [`Type::Sequence`] each
/// carry their cap, so a frontend that meets an unbounded source type must
/// resolve the bound (or reject the type) *before* it can build a `Type` — the
/// unboundedness can never reach the IR. See the [crate docs](crate) for why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    /// A primitive scalar.
    Scalar(Scalar),
    /// A length-bounded byte string holding at most `capacity` bytes.
    String {
        /// Maximum length in bytes.
        capacity: usize,
    },
    /// A fixed-length array of exactly `len` elements.
    Array {
        /// Element type.
        element: Box<Self>,
        /// Number of elements.
        len: usize,
    },
    /// A length-bounded sequence of at most `capacity` elements.
    Sequence {
        /// Element type.
        element: Box<Self>,
        /// Maximum number of elements.
        capacity: usize,
    },
    /// A reference to a named struct defined in the same module.
    Struct(TypeName),
    /// A reference to a named enum defined in the same module.
    Enum(TypeName),
}

impl Type {
    /// Convenience constructor for a scalar field.
    #[must_use]
    pub const fn scalar(s: Scalar) -> Self {
        Self::Scalar(s)
    }

    /// Convenience constructor for a bounded sequence.
    #[must_use]
    pub fn sequence(element: Self, capacity: usize) -> Self {
        Self::Sequence {
            element: Box::new(element),
            capacity,
        }
    }

    /// Convenience constructor for a fixed array.
    #[must_use]
    pub fn array(element: Self, len: usize) -> Self {
        Self::Array {
            element: Box::new(element),
            len,
        }
    }
}
