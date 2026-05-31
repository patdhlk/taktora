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

/// Sanitise an arbitrary product string into a valid Rust identifier (`REQ_0511`).
///
/// Each character that is not a valid Rust identifier character (`[A-Za-z0-9_]`)
/// is replaced with `_`. If the result would start with a digit it is prefixed
/// with `_`. Nothing is collapsed, so the mapping is faithful and stable: e.g.
/// `EL3001-0000` becomes `EL3001_0000`.
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
}
