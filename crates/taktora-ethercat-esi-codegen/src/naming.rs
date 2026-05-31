//! Naming and revision policy (`REQ_0511` / `REQ_0512`).
//!
//! This module is the single home of the rules that turn a parsed device's
//! product string into a stable Rust identifier and a revision-qualified
//! constant name. It knows nothing about token emission or backends.

use taktora_ethercat_esi as esi;

/// Pick the naming source string for a device (`REQ_0511`).
///
/// Preference order: the raw `<Type>` product string (`product_type`), then the
/// human `<Name>`, then a synthesised `device_<product_code:08X>`. The `<Type>`
/// string is the short device code (e.g. `EL3001-0000`) and is the canonical,
/// stable source.
fn naming_source(device: &esi::EsiDevice) -> String {
    device
        .product_type
        .as_deref()
        .or(device.name.as_deref())
        .map_or_else(
            || format!("device_{:08X}", device.identity.product_code),
            ToOwned::to_owned,
        )
}

/// Rust keywords (strict + reserved, covering the 2024 edition) that a sanitised
/// identifier must never equal, since emitting `pub struct match;` is invalid.
/// A sanitised string landing on any of these gets a trailing `_` (`match` →
/// `match_`); see [`sanitise_ident`].
const KEYWORDS: &[&str] = &[
    // Strict keywords.
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", // Reserved keywords.
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized",
    "virtual", "yield", "try", "gen",
];

/// Sanitise an arbitrary product string into a valid Rust identifier (`REQ_0511`).
///
/// Each character that is not a valid Rust identifier character (`[A-Za-z0-9_]`)
/// is replaced with `_`. If the result would start with a digit it is prefixed
/// with `_`. If the result lands exactly on a Rust keyword (strict or reserved)
/// a trailing `_` is appended (`match` → `match_`), since the derived ident is
/// emitted bare (e.g. `pub struct #ident;`) and raw idents (`r#`) can't express
/// every keyword (`crate`, `self`, `Self`). Nothing else is collapsed, so the
/// mapping stays faithful and stable: e.g. `EL3001-0000` becomes `EL3001_0000`.
fn sanitise_ident(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if out.is_empty() {
        out.push('_');
    } else if out.as_bytes()[0].is_ascii_digit() {
        out.insert(0, '_');
    }

    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }

    out
}

/// The sanitised base identifier for a device, before any revision suffix.
pub fn base_ident(device: &esi::EsiDevice) -> String {
    sanitise_ident(&naming_source(device))
}

/// The full-width revision suffix for a revision number (`REQ_0512`): `REV{rev:08X}`.
pub fn revision_suffix(revision: u32) -> String {
    format!("REV{revision:08X}")
}

/// The const identifier for a device: always `<SANITISED_UPPER>_REV<rev:08X>`.
pub fn const_ident_string(device: &esi::EsiDevice) -> String {
    format!(
        "{}_{}",
        base_ident(device).to_ascii_uppercase(),
        revision_suffix(device.identity.revision),
    )
}

/// The struct identifier for a device given whether its base ident collides
/// with another device in the set. On collision the `_REV<rev:08X>` suffix is
/// appended; otherwise the bare sanitised base ident is used.
pub fn struct_ident_string(device: &esi::EsiDevice, collides: bool) -> String {
    let base = base_ident(device);
    if collides {
        format!("{base}_{}", revision_suffix(device.identity.revision))
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitises_dash_to_underscore() {
        assert_eq!(sanitise_ident("EL3001-0000"), "EL3001_0000");
    }

    #[test]
    fn sanitises_dots_and_spaces() {
        assert_eq!(
            sanitise_ident("EL3001-like 1Ch. Ana."),
            "EL3001_like_1Ch__Ana_"
        );
    }

    #[test]
    fn prefixes_leading_digit() {
        assert_eq!(sanitise_ident("3001"), "_3001");
    }

    #[test]
    fn empty_string_becomes_underscore() {
        assert_eq!(sanitise_ident(""), "_");
    }

    #[test]
    fn revision_suffix_is_full_width_uppercase_hex() {
        assert_eq!(revision_suffix(0x0010_0000), "REV00100000");
        assert_eq!(revision_suffix(0x0011_0000), "REV00110000");
        assert_eq!(revision_suffix(0), "REV00000000");
        assert_eq!(revision_suffix(0xDEAD_BEEF), "REVDEADBEEF");
    }

    #[test]
    fn keyword_idents_get_trailing_underscore() {
        // Lowercase strict keywords would emit invalid `pub struct match;`.
        assert_eq!(sanitise_ident("match"), "match_");
        assert_eq!(sanitise_ident("fn"), "fn_");
        assert_eq!(sanitise_ident("loop"), "loop_");
        // `Self` is a keyword in its exact casing.
        assert_eq!(sanitise_ident("Self"), "Self_");
        // Reserved keywords are escaped too.
        assert_eq!(sanitise_ident("yield"), "yield_");
    }

    #[test]
    fn non_keywords_are_left_unchanged() {
        // Capitalised forms of keywords are not keywords.
        assert_eq!(sanitise_ident("Match"), "Match");
        // A keyword with extra characters is not a keyword.
        assert_eq!(sanitise_ident("matchbox"), "matchbox");
        assert_eq!(sanitise_ident("EL3001"), "EL3001");
    }

    /// Build a minimal device with the given naming-source fields; everything
    /// else is empty/zero so the test exercises only the source-selection path.
    fn device(product_type: Option<&str>, name: Option<&str>, product_code: u32) -> esi::EsiDevice {
        esi::EsiDevice {
            identity: esi::Identity {
                vendor_id: 0,
                product_code,
                revision: 0,
            },
            name: name.map(ToOwned::to_owned),
            product_type: product_type.map(ToOwned::to_owned),
            group_type: None,
            sync_managers: Vec::new(),
            tx_pdos: Vec::new(),
            rx_pdos: Vec::new(),
            mailbox: None,
            dc: None,
            dictionary: Vec::new(),
            vendor_extensions: Vec::new(),
        }
    }

    #[test]
    fn naming_source_prefers_product_type() {
        let dev = device(Some("EL3001-0000"), Some("Human Name"), 0x0BB9_3052);
        assert_eq!(naming_source(&dev), "EL3001-0000");
    }

    #[test]
    fn naming_source_falls_back_to_name() {
        // product_type None → use <Name>.
        let dev = device(None, Some("Human Name"), 0x0BB9_3052);
        assert_eq!(naming_source(&dev), "Human Name");
        assert_eq!(base_ident(&dev), "Human_Name");
    }

    #[test]
    fn naming_source_synthesises_from_product_code() {
        // Both None → synthesised device_<code:08X>.
        let dev = device(None, None, 0x0BB9_3052);
        assert_eq!(naming_source(&dev), "device_0BB93052");
        assert_eq!(base_ident(&dev), "device_0BB93052");
    }
}
