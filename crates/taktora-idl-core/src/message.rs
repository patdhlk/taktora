//! Aggregate types: structs, enums, and services.

use serde::{Deserialize, Serialize};

use crate::{Scalar, Type, TypeName};

/// One named field within a [`Struct`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// Field name, verbatim from the source description.
    pub name: String,
    /// Field type.
    pub ty: Type,
}

impl Field {
    /// Construct a field.
    pub fn new(name: impl Into<String>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// A composite record type: an ordered list of named fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Struct {
    /// Struct name, verbatim from the source description.
    pub name: String,
    /// Fields, in declaration order (serialization order).
    pub fields: Vec<Field>,
}

/// One `(name, value)` pair of an [`EnumDef`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumVariant {
    /// Variant name, verbatim from the source description.
    pub name: String,
    /// Discriminant value, widened to `i64` to hold any source representation.
    pub value: i64,
}

/// An enumeration carried on a fixed-width integer [`Scalar`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumDef {
    /// Enum name, verbatim from the source description.
    pub name: String,
    /// The integer scalar the discriminant is transmitted as.
    pub underlying: Scalar,
    /// The known variants. An on-wire value absent from this list is a
    /// decode-time concern for the backend, not an IR-level error.
    pub variants: Vec<EnumVariant>,
}

/// A request/reply service: the message-plane analogue of a method.
///
/// `response` is `None` for fire-and-forget (one-way) services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    /// Service name, verbatim from the source description.
    pub name: String,
    /// Request payload type.
    pub request: TypeName,
    /// Reply payload type, or `None` for a one-way service.
    pub response: Option<TypeName>,
}
