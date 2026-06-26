//! The closed POD [`FieldType`] descriptor set and the named [`FieldSchema`]
//! pairing (REQ_0858, REQ_0875).

use serde::{Deserialize, Serialize};

/// A closed, plain-old-data field-type descriptor.
///
/// These are *type descriptors*, not values — no float or string *values* are
/// ever stored here, so the type derives [`Eq`] without floating-point hazard.
/// The JSON form is internally tagged on `"type"` with `snake_case` tags and is
/// part of the cross-language wire contract (REQ_0858, REQ_0875).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldType {
    /// A boolean.
    Bool,
    /// Signed 8-bit integer.
    I8,
    /// Signed 16-bit integer.
    I16,
    /// Signed 32-bit integer.
    I32,
    /// Signed 64-bit integer.
    I64,
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
    /// 32-bit IEEE-754 float.
    F32,
    /// 64-bit IEEE-754 float.
    F64,
    /// A fixed-length array of `len` elements of `elem`.
    Array {
        /// The element type.
        elem: Box<FieldType>,
        /// The element count.
        len: u32,
    },
    /// An inline bounded UTF-8 string: a `len: u16` followed by `[u8; cap]`.
    Str {
        /// The byte capacity of the inline buffer.
        cap: u32,
    },
    /// A nested POD struct.
    Struct {
        /// The struct's fields, in declaration order.
        fields: Vec<FieldSchema>,
    },
    /// A C-like enum lowered to a backing integer of `width` bytes.
    Enum {
        /// The enum's Rust type name.
        name: String,
        /// The `(variant name, discriminant)` pairs, in declaration order.
        variants: Vec<(String, i64)>,
        /// The backing integer width in bytes.
        width: u8,
    },
}

/// A named field within a ViewModel or command-params schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSchema {
    /// The field name.
    pub name: String,
    /// The field type.
    #[serde(rename = "type")]
    pub ty: FieldType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_types_round_trip_and_use_tagged_json() {
        let f = FieldType::Array {
            elem: Box::new(FieldType::F64),
            len: 3,
        };
        let j = serde_json::to_value(&f).unwrap();
        assert_eq!(
            j,
            serde_json::json!({"type":"array","elem":{"type":"f64"},"len":3})
        );
        let s = FieldType::Str { cap: 32 };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"type":"str","cap":32})
        );
        let e = FieldType::Enum {
            name: "State".into(),
            variants: vec![("Idle".into(), 0), ("Run".into(), 1)],
            width: 1,
        };
        let back: FieldType = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }
}
