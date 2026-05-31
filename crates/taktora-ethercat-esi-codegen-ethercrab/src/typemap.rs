//! `DataType` → Rust type / bit read+write expression mapping.
//!
//! This module is the single home of the type-mapping rules. It is designed to
//! extend additively: each [`DataType`] variant resolves to a [`FieldType`]
//! describing the Rust type token and how to read/write the field from a
//! `bitvec` slice. Backends call [`resolve`] and assemble emission around it.

use proc_macro2::TokenStream;
use quote::quote;
use taktora_ethercat_esi::DataType;
use taktora_ethercat_esi_codegen::CodegenError;

/// The resolved Rust mapping for one PDO entry.
#[derive(Debug)]
pub struct FieldType {
    /// The Rust field type token (e.g. `bool`, `i16`, `f32`).
    pub rust_type: TokenStream,
}

impl FieldType {
    const fn new(rust_type: TokenStream) -> Self {
        Self { rust_type }
    }
}

/// How a field is laid out in the bit stream, used to build read/write exprs.
#[derive(Debug, Clone, Copy)]
pub enum Layout {
    /// A single bit read as `bool` at the given absolute bit offset.
    Bool { offset: usize },
    /// A multi-bit field occupying `offset..offset + width`, loaded via
    /// `load_le` into the storage type and then reinterpreted as `kind`.
    Field {
        offset: usize,
        width: usize,
        kind: ScalarKind,
    },
}

/// The reinterpretation applied to the little-endian loaded bits of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarKind {
    /// Loaded directly as the Rust integer type (signed or unsigned).
    Int,
    /// Loaded as `u32`, then `f32::from_bits`.
    F32,
    /// Loaded as `u64`, then `f64::from_bits`.
    F64,
}

/// Resolve a (parsed) data type plus its declared bit length into a Rust field
/// type and a layout describing the read/write.
///
/// `index`/`sub_index`/`field` are only used to build a descriptive
/// [`CodegenError::UnsupportedEntryType`] for types this backend cannot map.
///
/// # Errors
///
/// Returns [`CodegenError::UnsupportedEntryType`] for string and complex/named
/// types, which have no scalar Rust field representation in this slice.
pub fn resolve(
    data_type: Option<&DataType>,
    bit_length: u16,
    offset: usize,
    index: u16,
    sub_index: u8,
    field: &str,
) -> Result<(FieldType, Layout), CodegenError> {
    let unsupported = |reason: &str| CodegenError::UnsupportedEntryType {
        index,
        sub_index,
        field: field.to_owned(),
        reason: reason.to_owned(),
    };
    let width = bit_length as usize;

    // Build a signed/unsigned integer mapping over the next-larger storage
    // width, masking sub-width fields via `load_le`.
    let int = |signed: bool| -> (FieldType, Layout) {
        let store_bits = storage_bits(width);
        let ty = int_type_token(signed, store_bits);
        (
            FieldType::new(ty),
            Layout::Field {
                offset,
                width,
                kind: ScalarKind::Int,
            },
        )
    };

    match data_type {
        Some(DataType::Bool) => Ok((FieldType::new(quote! { bool }), Layout::Bool { offset })),
        Some(DataType::I8) => Ok(exact_int(true, 8, offset)),
        Some(DataType::I16) => Ok(exact_int(true, 16, offset)),
        Some(DataType::I32) => Ok(exact_int(true, 32, offset)),
        Some(DataType::I64) => Ok(exact_int(true, 64, offset)),
        Some(DataType::U8) => Ok(exact_int(false, 8, offset)),
        Some(DataType::U16) => Ok(exact_int(false, 16, offset)),
        Some(DataType::U32) => Ok(exact_int(false, 32, offset)),
        Some(DataType::U64) => Ok(exact_int(false, 64, offset)),
        Some(DataType::Real32) => Ok((
            FieldType::new(quote! { f32 }),
            Layout::Field {
                offset,
                width: 32,
                kind: ScalarKind::F32,
            },
        )),
        Some(DataType::Real64) => Ok((
            FieldType::new(quote! { f64 }),
            Layout::Field {
                offset,
                width: 64,
                kind: ScalarKind::F64,
            },
        )),
        // BitN and any non-standard width: next-larger unsigned via load_le.
        Some(DataType::BitN(_)) => Ok(int(false)),
        // Untyped but sized: width-inferred.
        None => {
            if width == 1 {
                Ok((FieldType::new(quote! { bool }), Layout::Bool { offset }))
            } else {
                Ok(int(false))
            }
        }
        Some(DataType::VisibleString) => Err(unsupported("VisibleString")),
        Some(DataType::OctetString) => Err(unsupported("OctetString")),
        Some(DataType::UnicodeString) => Err(unsupported("UnicodeString")),
        Some(DataType::Named(name)) => Err(unsupported(name)),
    }
}

/// An exactly-sized integer (`width` equals the declared `CoE` width).
fn exact_int(signed: bool, width: usize, offset: usize) -> (FieldType, Layout) {
    (
        FieldType::new(int_type_token(signed, width)),
        Layout::Field {
            offset,
            width,
            kind: ScalarKind::Int,
        },
    )
}

/// The next power-of-two storage width (in bits) that holds `width` bits,
/// clamped to 8..=64. A 1-bit field still rounds up to a `u8`-backed load.
const fn storage_bits(width: usize) -> usize {
    match width {
        0..=8 => 8,
        9..=16 => 16,
        17..=32 => 32,
        _ => 64,
    }
}

/// The Rust integer type token for the given storage width and signedness.
fn int_type_token(signed: bool, store_bits: usize) -> TokenStream {
    match (signed, store_bits) {
        (true, 8) => quote! { i8 },
        (true, 16) => quote! { i16 },
        (true, 32) => quote! { i32 },
        (true, _) => quote! { i64 },
        (false, 8) => quote! { u8 },
        (false, 16) => quote! { u16 },
        (false, 32) => quote! { u32 },
        (false, _) => quote! { u64 },
    }
}

impl Layout {
    /// The number of bits this field occupies in the stream.
    pub const fn width(&self) -> usize {
        match *self {
            Self::Bool { .. } => 1,
            Self::Field { width, .. } => width,
        }
    }
}
