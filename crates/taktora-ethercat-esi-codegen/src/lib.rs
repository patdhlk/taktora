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
use taktora_ethercat_esi::{AlternativeSmMapping, EsiFile, Pdo};

pub use taktora_fieldbus_od_core::Identity;

pub mod naming;

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
    /// Predefined PDO-assignment combinations (`AlternativeSmMapping`),
    /// borrowed from the parsed model. Empty for single-assignment devices.
    pub alt_sm_mappings: &'a [AlternativeSmMapping],
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

    /// A PDO-assignment alternative group resolved to zero alternatives. A
    /// backend cannot emit a choice enum with no variants (nor a `Default`), so
    /// this is surfaced rather than emitting uncompilable tokens. The grouping
    /// only ever produces non-empty groups, so this guards an internal
    /// invariant (`REQ_0523`).
    #[error("alternative group for enum {enum_ident:?} has no alternatives")]
    EmptyAlternativeGroup {
        /// The enum identifier the empty group would have produced.
        enum_ident: String,
    },

    /// A single direction (`Tx`/`Rx`) resolved to MORE THAN ONE PDO-assignment
    /// alternative group. The spec-required shape is one alternative group per
    /// direction (the master picks one PDO assignment via 0x1C12/0x1C13).
    /// Emitting more than one group is currently miscompiled: every group is
    /// laid out at the same `base_offset`, so two groups in one direction would
    /// alias the same bits at decode/encode time (silent data corruption).
    /// Until per-group offset threading exists this is rejected as a hard error
    /// rather than emitting wrong codegen (`REQ_0523`/`REQ_0524`).
    #[error(
        "device {device:?} resolves more than one alternative group in the {direction} direction; \
         multiple alternative groups per direction are not yet supported"
    )]
    MultipleAlternativeGroups {
        /// The device whose direction over-resolved.
        device: String,
        /// The offending direction (`"Tx"` for inputs, `"Rx"` for outputs).
        direction: &'static str,
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

/// Resolve a PDO into its `snake_case` device-struct field identifier
/// (`REQ_0511`), used when a multi-PDO device gets one sub-struct field per PDO.
///
/// Named PDOs derive from `<Name>` (`"Channel 1"` → `channel_1`); an unnamed PDO
/// falls back to its mapping index (`0x1A00` → `pdo_1a00`).
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the sanitised string does not lex
/// to exactly one identifier token (the sanitisation policy prevents this).
pub fn pdo_field_ident(name: Option<&str>, index: u16) -> Result<Ident, CodegenError> {
    make_ident(&naming::pdo_field_string(name, index))
}

/// Resolve a per-PDO sub-struct identifier as `<device_struct><PdoSegment>`
/// (`REQ_0511`).
///
/// The segment is a PascalCase-ish rendering of the PDO `<Name>` (`"Channel 1"`
/// → `Channel1`, giving `EL2004Channel1`) or, when unnamed, its mapping index
/// (`0x1A00` → `Pdo1a00`).
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the concatenation does not lex to
/// exactly one identifier token (the sanitisation policy prevents this).
pub fn pdo_struct_ident(
    device_struct: &Ident,
    name: Option<&str>,
    index: u16,
) -> Result<Ident, CodegenError> {
    let segment = naming::pdo_struct_segment(name, index);
    make_ident(&format!("{device_struct}{segment}"))
}

/// Resolve the enum identifier for a PDO-assignment alternative group
/// (`REQ_0523`).
///
/// A device with a single alternative group names it `<Dev>PdoAssignment`
/// (`label: None`). A device with multiple groups disambiguates each by a
/// caller-supplied `PascalCase` label segment (e.g. derived from the shared sync
/// manager / direction), yielding `<Dev>PdoAssignment<Label>`.
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the concatenation does not lex to
/// exactly one identifier token (the sanitisation policy prevents this).
pub fn pdo_assignment_enum_ident(
    device_struct: &Ident,
    label: Option<&str>,
) -> Result<Ident, CodegenError> {
    let suffix = label.map_or_else(String::new, naming::pdo_struct_segment_raw);
    make_ident(&format!("{device_struct}PdoAssignment{suffix}"))
}

/// Resolve the device-struct field identifier holding a PDO-assignment enum
/// (`REQ_0523`).
///
/// A single alternative group uses the bare `pdo` field; multiple groups
/// disambiguate with a caller-supplied snake-case label (`pdo_sm3`).
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the result does not lex to exactly
/// one identifier token (the sanitisation policy prevents this).
pub fn pdo_assignment_field_ident(label: Option<&str>) -> Result<Ident, CodegenError> {
    label.map_or_else(
        || make_ident("pdo"),
        |label| field_ident(&format!("pdo_{label}")),
    )
}

/// Resolve the enum-variant identifier for one alternative PDO (`REQ_0523`).
///
/// The variant is a `PascalCase` rendering of the PDO `<Name>` (`"Standard"` →
/// `Standard`), or its mapping index when unnamed (`0x1A00` → `Pdo1a00`).
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the rendered segment does not lex
/// to exactly one identifier token (the sanitisation policy prevents this).
pub fn pdo_variant_ident(name: Option<&str>, index: u16) -> Result<Ident, CodegenError> {
    make_ident(&naming::pdo_variant_segment(name, index))
}

/// Resolve the per-variant struct identifier `<Dev>Pdo<Variant>` holding one
/// alternative PDO's typed entries (`REQ_0524`).
///
/// The variant segment renders the PDO `<Name>` in `PascalCase` (`ALT` +
/// `Standard` → `ALTPdoStandard`), or its index when unnamed.
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the concatenation does not lex to
/// exactly one identifier token (the sanitisation policy prevents this).
pub fn pdo_variant_struct_ident(
    device_struct: &Ident,
    name: Option<&str>,
    index: u16,
) -> Result<Ident, CodegenError> {
    let segment = naming::pdo_variant_segment(name, index);
    make_ident(&format!("{device_struct}Pdo{segment}"))
}

/// The `<Dev>OpMode` enum identifier for a device's PDO-assignment set.
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the concatenation does not lex to
/// exactly one identifier token (the sanitisation policy prevents this).
pub fn op_mode_enum_ident(device_struct: &Ident) -> Result<Ident, CodegenError> {
    make_ident(&format!("{device_struct}OpMode"))
}

/// The enum-variant identifier for one assignment (`<Dev>OpMode::<Variant>`).
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the rendered segment does not lex to
/// exactly one identifier token (the sanitisation policy prevents this).
pub fn op_mode_variant_ident(name: Option<&str>, ordinal: usize) -> Result<Ident, CodegenError> {
    make_ident(&naming::op_mode_variant_segment(name, ordinal))
}

/// The per-variant data-struct identifier (`<Dev><Variant>`).
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIdent`] if the concatenation does not lex to
/// exactly one identifier token (the sanitisation policy prevents this).
pub fn op_mode_variant_struct_ident(
    device_struct: &Ident,
    name: Option<&str>,
    ordinal: usize,
) -> Result<Ident, CodegenError> {
    make_ident(&format!(
        "{device_struct}{}",
        naming::op_mode_variant_segment(name, ordinal)
    ))
}

/// Whether every resolved device has a distinct `const_ident`. Used only by the
/// debug-assert guarding the (base ident, revision)-uniqueness invariant in
/// [`resolve_devices`]; isolated so the assertion expression stays side-effect
/// free.
///
/// NOT `#[cfg(debug_assertions)]`-gated: `debug_assert!` expands to
/// `if cfg!(debug_assertions) { … }`, whose body is type-checked even in release
/// builds, so this function must exist in release too (the `if false` reference
/// keeps it from tripping `dead_code`).
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
                alt_sm_mappings: &device.alt_sm_mappings,
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
