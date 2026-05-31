//! Codegen layer for the `EtherCAT` ESI device-driver toolchain.
//!
//! This crate owns the naming / collision policy (`REQ_0511`, `REQ_0512`), the
//! [`CodegenBackend`] trait, and the [`generate`] entry point. It knows nothing
//! about XML parsing or any concrete runtime: it consumes the parsed model from
//! [`taktora_ethercat_esi`], applies all naming policy to produce a resolved,
//! borrowing [`Device`] IR, and lets a backend emit [`proc_macro2::TokenStream`]
//! per device plus a module root.
//!
//! The split is deliberate: policy lives here so backends stay policy-free
//! (`REQ_0511`). Emission is `quote!`-assembled token streams (`REQ_0514`); this
//! crate never formats source (prettyplease lives downstream).

use std::collections::HashMap;

use proc_macro2::{Ident, TokenStream};
use taktora_ethercat_esi::{EsiFile, Pdo};

pub use taktora_fieldbus_od_core::Identity;

mod naming;

/// A resolved, borrowing codegen IR for one device: parser data with all naming
/// and revision policy already applied.
///
/// Backends receive `&Device` and never re-derive identifiers; everything they
/// need for policy-correct emission is already resolved here (`REQ_0511`).
pub struct Device<'a> {
    /// Sanitised product identifier. Carries the `_REV<rev:08X>` suffix **only**
    /// when its base ident collides with another device in the set (`REQ_0512`).
    pub struct_ident: Ident,
    /// Constant identifier: always `<SANITISED_UPPER>_REV<rev:08X>` (`REQ_0512`).
    pub const_ident: Ident,
    /// Device identity (vendor / product / revision).
    pub identity: Identity,
    /// Human-readable device name (`<Name>`), when present.
    pub name: Option<&'a str>,
    /// `TxPDOs` (`SubDevice` → master), borrowed from the parsed model.
    pub tx_pdos: &'a [Pdo],
    /// `RxPDOs` (master → `SubDevice`), borrowed from the parsed model.
    pub rx_pdos: &'a [Pdo],
}

/// A backend that turns resolved [`Device`]s into Rust token streams.
///
/// Implementors are policy-free: all naming/collision decisions are already
/// baked into the [`Device`] handed to them (`REQ_0511`).
pub trait CodegenBackend {
    /// Emit the tokens for a single device.
    fn emit_device(&self, device: &Device) -> Result<TokenStream, CodegenError>;

    /// Emit the module-root tokens for the whole device set (e.g. a registry,
    /// re-exports, or shared preamble).
    fn emit_module_root(&self, devices: &[Device]) -> Result<TokenStream, CodegenError>;
}

/// Errors raised while resolving the codegen IR or emitting tokens.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodegenError {
    /// A resolved identifier was not a single valid Rust identifier token. This
    /// should not occur given the sanitisation policy, but is surfaced rather
    /// than panicked (`Ident::new` would panic on such input).
    #[error("invalid generated identifier {ident:?}: {reason}")]
    InvalidIdent {
        /// The offending identifier string.
        ident: String,
        /// Why the string is not a usable identifier (lexing failure, or it did
        /// not lex to exactly one bare identifier token).
        reason: String,
    },

    /// A PDO entry used a data type the backend cannot yet emit a typed field
    /// for (e.g. a string or a complex/named type). Reported rather than
    /// silently dropped so the device author sees the gap.
    #[error("unsupported entry data type for entry {index:#06x}:{sub_index} ({field}): {reason}")]
    UnsupportedEntryType {
        /// PDO entry index.
        index: u16,
        /// PDO entry sub-index.
        sub_index: u8,
        /// The resolved field name the entry would have produced.
        field: String,
        /// Why the type is unsupported (the offending type name / category).
        reason: String,
    },
}

/// Parse an identifier string into a [`proc_macro2::Ident`], surfacing failures
/// as [`CodegenError::InvalidIdent`] rather than panicking.
///
/// `Ident::new` panics on invalid input, so we first validate by lexing the
/// string and confirming it is exactly one identifier token. Anything that does
/// not lex to a single bare identifier (multiple tokens, a literal, a keyword
/// token, punctuation) yields [`CodegenError::InvalidIdent`] — we never fall
/// back to the panicking `Ident::new`.
fn make_ident(s: &str) -> Result<Ident, CodegenError> {
    use std::str::FromStr as _;

    let ts = TokenStream::from_str(s).map_err(|source| CodegenError::InvalidIdent {
        ident: s.to_owned(),
        reason: source.to_string(),
    })?;
    let mut iter = ts.into_iter();
    match (iter.next(), iter.next()) {
        (Some(proc_macro2::TokenTree::Ident(ident)), None) => Ok(ident),
        _ => Err(CodegenError::InvalidIdent {
            ident: s.to_owned(),
            reason: "did not lex to exactly one identifier token".to_owned(),
        }),
    }
}

/// Resolve a raw PDO-entry name into a `snake_case` Rust field identifier,
/// applying this crate's naming policy (`REQ_0511`).
///
/// The raw string is word-segmented and lower-cased, then char-sanitised and
/// keyword-escaped through the same rules used for type identifiers, so the
/// result is always a valid bare identifier (e.g. `Underrange` → `underrange`,
/// `Value` → `value`). Backends call this rather than re-deriving naming.
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the sanitised string somehow does
/// not lex to exactly one identifier token — which the sanitisation policy is
/// designed to prevent, but is surfaced rather than panicked.
pub fn field_ident(raw: &str) -> Result<Ident, CodegenError> {
    make_ident(&naming::snake_field_string(raw))
}

/// Whether every resolved device has a distinct `const_ident`. Used only by the
/// debug-assert guarding the (base ident, revision)-uniqueness invariant in
/// [`resolve_devices`]; isolated so the assertion expression stays side-effect
/// free.
#[cfg(debug_assertions)]
fn const_idents_unique(devices: &[Device<'_>]) -> bool {
    let mut consts: Vec<String> = devices.iter().map(|d| d.const_ident.to_string()).collect();
    consts.sort_unstable();
    let len = consts.len();
    consts.dedup();
    consts.len() == len
}

/// Resolve all parsed devices into borrowing [`Device`]s, applying naming
/// (`REQ_0511`) and revision/collision policy (`REQ_0512`) across the whole set.
///
/// Collisions are detected by counting base idents over the entire input first,
/// so the resolved identifiers are independent of `esi.devices` order.
fn resolve_devices(esi: &EsiFile) -> Result<Vec<Device<'_>>, CodegenError> {
    let mut base_counts: HashMap<String, usize> = HashMap::new();
    for device in &esi.devices {
        *base_counts.entry(naming::base_ident(device)).or_insert(0) += 1;
    }

    // INVARIANT: (base ident, revision) is assumed unique within an input set.
    // Two devices sharing BOTH would resolve to identical `struct_ident` AND
    // `const_ident`, emitting duplicate `pub struct`/`pub const` that won't
    // compile. We do not detect or merge true structural duplicates here;
    // that dedup is deferred to the dedup slice (`REQ_0513`).
    let resolved: Vec<Device<'_>> = esi
        .devices
        .iter()
        .map(|device| {
            let collides = base_counts[&naming::base_ident(device)] > 1;
            Ok(Device {
                struct_ident: make_ident(&naming::struct_ident_string(device, collides))?,
                const_ident: make_ident(&naming::const_ident_string(device))?,
                identity: device.identity,
                name: device.name.as_deref(),
                tx_pdos: &device.tx_pdos,
                rx_pdos: &device.rx_pdos,
            })
        })
        .collect::<Result<_, _>>()?;

    debug_assert!(
        const_idents_unique(&resolved),
        "(base ident, revision) collision: two resolved devices share a const ident"
    );

    Ok(resolved)
}

/// Build resolved [`Device`]s from a parsed ESI file and emit the full module.
///
/// Applies naming (`REQ_0511`) and revision/collision policy (`REQ_0512`) across the
/// whole set, then concatenates `emit_device` per device followed by a single
/// `emit_module_root`. Resolved idents are order-independent.
pub fn generate<B: CodegenBackend>(
    esi: &EsiFile,
    backend: &B,
) -> Result<TokenStream, CodegenError> {
    let devices = resolve_devices(esi)?;
    let mut ts = TokenStream::new();
    for device in &devices {
        ts.extend(backend.emit_device(device)?);
    }
    ts.extend(backend.emit_module_root(&devices)?);
    Ok(ts)
}
