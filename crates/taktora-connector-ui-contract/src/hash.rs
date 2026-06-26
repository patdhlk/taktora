//! The deterministic, order-independent structural contract hash (REQ_0874).
//!
//! # Canonical encoding (the cross-language spec)
//!
//! The hash is `lowercase_hex(SHA-256(canonical_utf8))`, where `canonical_utf8`
//! is built deterministically from the *structure* of a [`Manifest`] only.
//! Instance-specific fields are **excluded**: `instance`, `epoch`,
//! `contract_hash`, and every service name (`ViewModelSchema::service`,
//! `CommandSchema::request_service` / `reply_service` / `can_execute_service`)
//! — these depend on the instance namespace and must not affect compatibility.
//!
//! Any other language reproducing this hash MUST emit byte-identical
//! `canonical_utf8`. The grammar is:
//!
//! ```text
//! manifest   := viewmodels commands
//! viewmodels := "VM:" name "{" field (";" field)* "}"   for each VM, VMs sorted by name
//! field      := name ":" fieldtype
//! commands   := "CMD:" name "(" param (";" param)* ")" "kind=" kind "idem=" bool
//!                                                       for each command, commands sorted by name
//! param      := name ":" fieldtype                      (params sorted by name)
//! ```
//!
//! Fields and params are sorted by `name` (UTF-8 byte order) before encoding,
//! so vector order does not affect the hash. `bool` is `true`/`false`. `kind`
//! is the lowercase wire tag (`property`/`command`/`can_execute`/`event`).
//!
//! `fieldtype` is encoded recursively:
//!
//! ```text
//! bool | i8 | i16 | i32 | i64 | u8 | u16 | u32 | u64 | f32 | f64
//! array<ELEM;LEN>
//! str<CAP>
//! struct{ field (";" field)* }          (nested fields sorted by name)
//! enum:NAME<WIDTH>(VARNAME=DISC,...)     (variants sorted by (DISC, VARNAME))
//! ```
//!
//! # Name preconditions (grammar safety)
//!
//! The canonical encoding writes names raw and uses `:`, `;`, `{`, `}`, `<`,
//! `>`, `=`, and `,` as structural delimiters with **no escaping**. To keep the
//! grammar unambiguous — so two structurally-distinct manifests can never
//! collide by smuggling a delimiter into a name — every name in a manifest
//! (ViewModel, command, field, param, enum type, enum variant) MUST match
//! `^[A-Za-z0-9_]+$`. This charset contains none of the delimiters, so the
//! encoding is injective over valid inputs. [`validate_name`] is the predicate;
//! [`contract_hash`] `debug_assert!`s it at every name-encoding site.
//!
//! Names are additionally **unique within their section**: ViewModel names are
//! unique across the manifest, command names are unique across the manifest, and
//! field/param names are unique within a single schema (so the by-name sort is
//! total and input-order-independent). [`contract_hash`] `debug_assert!`s these
//! too. Both preconditions are contract obligations of the manifest author.

use std::fmt::Write;

use sha2::{Digest, Sha256};

use crate::field::{FieldSchema, FieldType};
use crate::kind::Kind;
use crate::schema::Manifest;

/// Returns `true` iff `name` is a valid contract identifier — non-empty and
/// matching `^[A-Za-z0-9_]+$`.
///
/// All names in a [`Manifest`] (ViewModel, command, field, param, enum type,
/// enum variant) MUST satisfy this. The charset deliberately excludes every
/// structural delimiter used by the canonical hash encoding (`:`, `;`, `{`,
/// `}`, `<`, `>`, `=`, `,`), guaranteeing the encoding is unambiguous and that
/// two distinct contracts cannot collide via an embedded delimiter.
#[must_use]
pub fn validate_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn kind_tag(kind: Kind) -> &'static str {
    match kind {
        Kind::Property => "property",
        Kind::Command => "command",
        Kind::CanExecute => "can_execute",
        Kind::Event => "event",
    }
}

fn encode_field_type(ty: &FieldType, out: &mut String) {
    match ty {
        FieldType::Bool => out.push_str("bool"),
        FieldType::I8 => out.push_str("i8"),
        FieldType::I16 => out.push_str("i16"),
        FieldType::I32 => out.push_str("i32"),
        FieldType::I64 => out.push_str("i64"),
        FieldType::U8 => out.push_str("u8"),
        FieldType::U16 => out.push_str("u16"),
        FieldType::U32 => out.push_str("u32"),
        FieldType::U64 => out.push_str("u64"),
        FieldType::F32 => out.push_str("f32"),
        FieldType::F64 => out.push_str("f64"),
        FieldType::Array { elem, len } => {
            out.push_str("array<");
            encode_field_type(elem, out);
            out.push(';');
            let _ = write!(out, "{len}");
            out.push('>');
        }
        FieldType::Str { cap } => {
            out.push_str("str<");
            let _ = write!(out, "{cap}");
            out.push('>');
        }
        FieldType::Struct { fields } => {
            out.push_str("struct{");
            encode_fields(fields, out);
            out.push('}');
        }
        FieldType::Enum {
            name,
            variants,
            width,
        } => {
            debug_assert!(
                validate_name(name),
                "enum type name {name:?} must match ^[A-Za-z0-9_]+$"
            );
            out.push_str("enum:");
            out.push_str(name);
            out.push('<');
            let _ = write!(out, "{width}");
            out.push_str(">(");
            let mut sorted: Vec<&(String, i64)> = variants.iter().collect();
            sorted.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            for (i, v) in sorted.iter().enumerate() {
                debug_assert!(
                    validate_name(&v.0),
                    "enum variant name {:?} must match ^[A-Za-z0-9_]+$",
                    v.0
                );
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&v.0);
                out.push('=');
                let _ = write!(out, "{}", v.1);
            }
            out.push(')');
        }
    }
}

/// Encode a field list, sorted by name, as `name:type;name:type;...`.
fn encode_fields(fields: &[FieldSchema], out: &mut String) {
    let mut sorted: Vec<&FieldSchema> = fields.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    debug_assert!(
        sorted.windows(2).all(|w| w[0].name != w[1].name),
        "duplicate field/param name within a schema"
    );
    for (i, f) in sorted.iter().enumerate() {
        debug_assert!(
            validate_name(&f.name),
            "field/param name {:?} must match ^[A-Za-z0-9_]+$",
            f.name
        );
        if i > 0 {
            out.push(';');
        }
        out.push_str(&f.name);
        out.push(':');
        encode_field_type(&f.ty, out);
    }
}

/// Build the canonical UTF-8 encoding of a manifest's structure.
fn canonical_encoding(m: &Manifest) -> String {
    let mut out = String::new();

    let mut vms: Vec<&_> = m.view_models.iter().collect();
    vms.sort_by(|a, b| a.name.cmp(&b.name));
    debug_assert!(
        vms.windows(2).all(|w| w[0].name != w[1].name),
        "duplicate ViewModel name"
    );
    for vm in vms {
        debug_assert!(
            validate_name(&vm.name),
            "ViewModel name {:?} must match ^[A-Za-z0-9_]+$",
            vm.name
        );
        out.push_str("VM:");
        out.push_str(&vm.name);
        out.push('{');
        encode_fields(&vm.fields, &mut out);
        out.push('}');
    }

    let mut cmds: Vec<&_> = m.commands.iter().collect();
    cmds.sort_by(|a, b| a.name.cmp(&b.name));
    debug_assert!(
        cmds.windows(2).all(|w| w[0].name != w[1].name),
        "duplicate command name"
    );
    for cmd in cmds {
        debug_assert!(
            validate_name(&cmd.name),
            "command name {:?} must match ^[A-Za-z0-9_]+$",
            cmd.name
        );
        out.push_str("CMD:");
        out.push_str(&cmd.name);
        out.push('(');
        encode_fields(&cmd.params, &mut out);
        out.push_str(")kind=");
        out.push_str(kind_tag(cmd.kind));
        out.push_str("idem=");
        out.push_str(if cmd.idempotent { "true" } else { "false" });
    }

    out
}

/// Compute the deterministic, order-independent structural contract hash of a
/// manifest as lowercase hex SHA-256 (REQ_0874).
///
/// See the module docs for the exact canonical encoding other languages must
/// reproduce. The hash excludes `instance`, `epoch`, `contract_hash`, and all
/// service names.
#[must_use]
pub fn contract_hash(m: &Manifest) -> String {
    let canonical = canonical_encoding(m);
    let digest = Sha256::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{FieldSchema, FieldType};
    use crate::schema::{Manifest, ViewModelSchema};

    fn vm(name: &str) -> ViewModelSchema {
        ViewModelSchema {
            name: name.into(),
            // Service names are instance-namespaced and therefore excluded from
            // the structural hash; vary them to prove they do not contribute.
            service: format!("inst/vm/{name}"),
            fields: vec![FieldSchema {
                name: "position".into(),
                ty: FieldType::F64,
            }],
        }
    }

    fn vm_with_extra_field(name: &str) -> ViewModelSchema {
        let mut v = vm(name);
        v.fields.push(FieldSchema {
            name: "velocity".into(),
            ty: FieldType::F64,
        });
        v
    }

    fn manifest_with(view_models: Vec<ViewModelSchema>) -> Manifest {
        Manifest {
            instance: "inst".into(),
            epoch: 1,
            contract_hash: String::new(),
            view_models,
            commands: vec![],
        }
    }

    #[test]
    fn hash_is_order_independent_and_structural() {
        let a = manifest_with(vec![vm("A"), vm("B")]);
        let b = manifest_with(vec![vm("B"), vm("A")]);
        assert_eq!(contract_hash(&a), contract_hash(&b));
        let c = manifest_with(vec![vm_with_extra_field("A"), vm("B")]);
        assert_ne!(contract_hash(&a), contract_hash(&c));
    }

    #[test]
    fn hash_excludes_instance_epoch_and_self() {
        let base = manifest_with(vec![vm("A")]);
        let mut other = base.clone();
        other.instance = "totally-different".into();
        other.epoch = 9999;
        other.contract_hash = "stale".into();
        // The service name on the ViewModel also differs structure-irrelevant.
        other.view_models[0].service = "totally-different/vm/A".into();
        assert_eq!(contract_hash(&base), contract_hash(&other));
    }

    #[test]
    fn hash_is_lowercase_hex_sha256() {
        let h = contract_hash(&manifest_with(vec![vm("A")]));
        assert_eq!(h.len(), 64);
        assert!(
            h.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn empty_manifest_hashes_deterministically() {
        let m = manifest_with(vec![]);
        let h = contract_hash(&m);
        // The canonical encoding of an empty manifest is the empty string, so the
        // hash is a stable, well-known constant: SHA-256("").
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // Recomputing is stable (no panic, identical hex).
        assert_eq!(contract_hash(&m), h);
    }

    /// A name embedding any canonical-grammar delimiter is rejected, so two
    /// structurally-distinct manifests cannot collide by smuggling a delimiter
    /// into a name (e.g. a VM literally named `A{x:f64}` versus a VM `A` with a
    /// field `x`). The validator is the guard that makes such names invalid.
    #[test]
    fn validate_name_rejects_delimiter_bearing_names() {
        for ok in ["A", "Stepper", "jog_relative", "field0", "_x", "U64"] {
            assert!(validate_name(ok), "{ok:?} should be valid");
        }
        for bad in [
            "", "A:B", "A;B", "A{B", "A}B", "A<B", "A>B", "A=B", "A,B", "a b", "ä",
        ] {
            assert!(!validate_name(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn enum_variant_input_order_does_not_affect_hash() {
        let mk = |variants: Vec<(String, i64)>| {
            let mut v = vm("A");
            v.fields.push(FieldSchema {
                name: "state".into(),
                ty: FieldType::Enum {
                    name: "State".into(),
                    variants,
                    width: 1,
                },
            });
            manifest_with(vec![v])
        };
        let a = mk(vec![
            ("Idle".into(), 0),
            ("Running".into(), 1),
            ("Faulted".into(), 2),
        ]);
        let b = mk(vec![
            ("Faulted".into(), 2),
            ("Idle".into(), 0),
            ("Running".into(), 1),
        ]);
        assert_eq!(contract_hash(&a), contract_hash(&b));
    }

    #[test]
    fn nested_field_types_participate_in_hash() {
        let with_ty = |ty: FieldType| {
            let mut v = vm("A");
            v.fields.push(FieldSchema {
                name: "x".into(),
                ty,
            });
            manifest_with(vec![v])
        };

        // Struct: changing a nested field's type changes the hash.
        let struct_a = with_ty(FieldType::Struct {
            fields: vec![FieldSchema {
                name: "inner".into(),
                ty: FieldType::F64,
            }],
        });
        let struct_b = with_ty(FieldType::Struct {
            fields: vec![FieldSchema {
                name: "inner".into(),
                ty: FieldType::I64,
            }],
        });
        assert_ne!(contract_hash(&struct_a), contract_hash(&struct_b));

        // Array: changing the element type changes the hash.
        let array_a = with_ty(FieldType::Array {
            elem: Box::new(FieldType::F64),
            len: 4,
        });
        let array_b = with_ty(FieldType::Array {
            elem: Box::new(FieldType::I64),
            len: 4,
        });
        assert_ne!(contract_hash(&array_a), contract_hash(&array_b));

        // Str: changing the capacity changes the hash.
        let str_a = with_ty(FieldType::Str { cap: 16 });
        let str_b = with_ty(FieldType::Str { cap: 32 });
        assert_ne!(contract_hash(&str_a), contract_hash(&str_b));

        // Enum: changing a variant changes the hash.
        let enum_a = with_ty(FieldType::Enum {
            name: "E".into(),
            variants: vec![("A".into(), 0)],
            width: 1,
        });
        let enum_b = with_ty(FieldType::Enum {
            name: "E".into(),
            variants: vec![("B".into(), 0)],
            width: 1,
        });
        assert_ne!(contract_hash(&enum_a), contract_hash(&enum_b));
    }
}
