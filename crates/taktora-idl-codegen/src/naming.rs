//! Identifier policy: the one place source names become Rust identifiers.
//!
//! The IR carries source names verbatim; turning `EngineData_Gear` into a type
//! `EngineDataGear` and `CoolantTemp` into a field `coolant_temp` is policy,
//! and it lives here so backends stay policy-free (`REQ_0954`) — exactly the
//! split the device-plane toolchain draws with its own `naming.rs`.

use proc_macro2::Ident;
use quote::format_ident;

/// Rust 2024 reserved words that cannot be bare identifiers. A field whose
/// snake-case form lands on one is suffixed with `_`.
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", "gen", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "union",
];

/// Split a source name into lowercase words, breaking on non-alphanumeric runs
/// and on lower/digit → upper case transitions (so `CoolantTemp` → `coolant`,
/// `temp`). Empty input yields a single `_` placeholder word.
fn words(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for ch in raw.chars() {
        if ch.is_alphanumeric() {
            if let Some(p) = prev {
                if (p.is_lowercase() || p.is_ascii_digit()) && ch.is_uppercase() && !cur.is_empty()
                {
                    out.push(core::mem::take(&mut cur));
                }
            }
            cur.extend(ch.to_lowercase());
        } else if !cur.is_empty() {
            out.push(core::mem::take(&mut cur));
        }
        prev = Some(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push("_".to_owned());
    }
    out
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// `PascalCase`d, identifier-safe string from a source name. A leading digit is
/// prefixed with `_`.
fn pascal(raw: &str) -> String {
    let joined: String = words(raw).iter().map(|w| capitalize(w)).collect();
    prefix_if_leading_digit(joined)
}

/// `snake_case`d, identifier-safe string from a source name.
fn snake(raw: &str) -> String {
    let joined = words(raw).join("_");
    let joined = prefix_if_leading_digit(joined);
    if KEYWORDS.contains(&joined.as_str()) {
        format!("{joined}_")
    } else {
        joined
    }
}

fn prefix_if_leading_digit(s: String) -> String {
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{s}")
    } else {
        s
    }
}

/// A `PascalCase` type identifier (struct or enum) from a source name.
#[must_use]
pub fn type_ident(raw: &str) -> Ident {
    format_ident!("{}", pascal(raw))
}

/// A `PascalCase` enum-variant identifier from a source name.
#[must_use]
pub fn variant_ident(raw: &str) -> Ident {
    format_ident!("{}", pascal(raw))
}

/// A `snake_case` field identifier from a source name, keyword-escaped.
#[must_use]
pub fn field_ident(raw: &str) -> Ident {
    format_ident!("{}", snake(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_names_pascalize() {
        assert_eq!(type_ident("EngineData").to_string(), "EngineData");
        assert_eq!(type_ident("EngineData_Gear").to_string(), "EngineDataGear");
        assert_eq!(type_ident("body-control").to_string(), "BodyControl");
    }

    #[test]
    fn field_names_snakeize() {
        assert_eq!(field_ident("Rpm").to_string(), "rpm");
        assert_eq!(field_ident("CoolantTemp").to_string(), "coolant_temp");
        assert_eq!(field_ident("DoorState").to_string(), "door_state");
    }

    #[test]
    fn keywords_and_leading_digits_are_escaped() {
        assert_eq!(field_ident("type").to_string(), "type_");
        assert_eq!(field_ident("4wd").to_string(), "_4wd");
        assert_eq!(type_ident("2fast").to_string(), "_2fast");
    }
}
