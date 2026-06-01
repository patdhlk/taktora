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
    /// An optional doc-comment line for the generated struct field. Set for
    /// width-inferred *opaque* mappings (a non-scalar / non-modelled `CoE` type
    /// resolved purely from its bit width), so the emitted code self-documents
    /// that the semantic type was not modelled. `None` for clean scalars.
    pub doc: Option<String>,
}

impl FieldType {
    const fn new(rust_type: TokenStream) -> Self {
        Self {
            rust_type,
            doc: None,
        }
    }

    const fn with_doc(rust_type: TokenStream, doc: String) -> Self {
        Self {
            rust_type,
            doc: Some(doc),
        }
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
    /// A byte-aligned opaque blob occupying `offset..offset + bytes * 8`,
    /// decoded as a fixed `[u8; bytes]` array (the bit range copied out of the
    /// slice) and encoded by copying it back. Used for width-inferred opaque
    /// fields wider than 64 bits, which no `uN` storage type can hold.
    Bytes { offset: usize, bytes: usize },
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
/// The type map is **total** for any entry with a usable `bit_length`
/// (`REQ` — full vendor corpus codegen): clean scalars map exactly; every other
/// sized entry (`BitN`, `Named(_)`, the string types, or a sized entry whose
/// declared `DataType` doesn't match a scalar) is resolved purely from its bit
/// width into an *opaque* field carrying a doc marker. This intentionally
/// reverses the earlier "error on strings / `Named`" decision (design Q7): the
/// goal of compiling the entire Beckhoff vendor corpus requires that an
/// unmodelled `CoE` type (e.g. `BITARR8`) never aborts codegen for the whole
/// file. The opaque width-inferred fallback preserves the bit layout faithfully
/// while leaving the semantic type unmodelled (and saying so in a doc-comment).
///
/// Width-inference precedence for the fallback:
/// 1. `width == 1` → `bool`.
/// 2. `width <= 64` → next-larger unsigned (`u8/u16/u32/u64`), masked via
///    `load_le`/`store_le`.
/// 3. `width > 64` → a fixed `[u8; ceil(width/8)]` byte array.
///
/// `index`/`sub_index`/`field` are only used to build a descriptive error for
/// the one genuinely-unusable case.
///
/// # Errors
///
/// Returns [`CodegenError::UnsupportedEntryType`] only when `bit_length` is
/// itself unusable — i.e. `0` for a non-padding entry (padding entries with
/// index 0 never reach here). For every sized entry the map is total.
pub fn resolve(
    data_type: Option<&DataType>,
    bit_length: u16,
    offset: usize,
    index: u16,
    sub_index: u8,
    field: &str,
) -> Result<(FieldType, Layout), CodegenError> {
    let width = bit_length as usize;

    match data_type {
        // Clean scalars map exactly, with no opaque doc marker.
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
        // BitN / untyped-but-sized: width-inferred, no opaque marker (a `BitN`
        // is already an honest "n raw bits", and an untyped entry was never
        // named, so there is nothing to flag as "unmodelled"). A 1-bit field is
        // a `bool`; wider widths use the next-larger unsigned via `load_le`.
        // The `BitN` ≥ 2 path stays byte-identical to the prior mapping.
        Some(DataType::BitN(_)) | None => {
            if width == 1 {
                Ok((FieldType::new(quote! { bool }), Layout::Bool { offset }))
            } else {
                Ok(width_unsigned(width, offset))
            }
        }
        // Opaque, unmodelled types: faithful width-inferred fallback + doc
        // marker. This is the Q7 reversal — these previously errored.
        Some(DataType::VisibleString) => {
            opaque(width, offset, "VisibleString", index, sub_index, field)
        }
        Some(DataType::OctetString) => {
            opaque(width, offset, "OctetString", index, sub_index, field)
        }
        Some(DataType::UnicodeString) => {
            opaque(width, offset, "UnicodeString", index, sub_index, field)
        }
        Some(DataType::Named(name)) => opaque(width, offset, name, index, sub_index, field),
    }
}

/// Resolve an opaque (unmodelled) type purely from its bit width, attaching a
/// `/// opaque <NAME> (<n> bits)` doc marker to the generated field.
///
/// # Errors
///
/// [`CodegenError::UnsupportedEntryType`] if `width == 0` (a non-padding entry
/// with no bits is genuinely unusable — it can be neither loaded nor stored).
fn opaque(
    width: usize,
    offset: usize,
    name: &str,
    index: u16,
    sub_index: u8,
    field: &str,
) -> Result<(FieldType, Layout), CodegenError> {
    if width == 0 {
        return Err(CodegenError::UnsupportedEntryType {
            index,
            sub_index,
            field: field.to_owned(),
            reason: format!("{name} has zero bit length"),
        });
    }

    let doc = format!("opaque {name} ({width} bits)");

    if width == 1 {
        return Ok((
            FieldType::with_doc(quote! { bool }, doc),
            Layout::Bool { offset },
        ));
    }

    if width <= 64 {
        let store_bits = storage_bits(width);
        let ty = int_type_token(false, store_bits);
        return Ok((
            FieldType::with_doc(ty, doc),
            Layout::Field {
                offset,
                width,
                kind: ScalarKind::Int,
            },
        ));
    }

    // > 64 bits: no `uN` holds it. Emit a byte array spanning the (byte-rounded)
    // bit range. `bit_length` need not be a byte multiple; the array covers
    // ceil(width/8) bytes and decode/encode copy that byte-aligned range.
    let bytes = width.div_ceil(8);
    Ok((
        FieldType::with_doc(quote! { [u8; #bytes] }, doc),
        Layout::Bytes { offset, bytes },
    ))
}

/// A width-inferred unsigned field: next-larger `uN` storage, masked via
/// `load_le`. Used for clean `BitN` / untyped-but-sized entries (no doc marker).
fn width_unsigned(width: usize, offset: usize) -> (FieldType, Layout) {
    let store_bits = storage_bits(width);
    let ty = int_type_token(false, store_bits);
    (
        FieldType::new(ty),
        Layout::Field {
            offset,
            width,
            kind: ScalarKind::Int,
        },
    )
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
            Self::Bytes { bytes, .. } => bytes * 8,
        }
    }
}
