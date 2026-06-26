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

use sha2::{Digest, Sha256};

use crate::field::{FieldSchema, FieldType};
use crate::kind::Kind;
use crate::schema::Manifest;

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
            out.push_str(&len.to_string());
            out.push('>');
        }
        FieldType::Str { cap } => {
            out.push_str("str<");
            out.push_str(&cap.to_string());
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
            out.push_str("enum:");
            out.push_str(name);
            out.push('<');
            out.push_str(&width.to_string());
            out.push_str(">(");
            let mut sorted = variants.clone();
            sorted.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            for (i, (vname, disc)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(vname);
                out.push('=');
                out.push_str(&disc.to_string());
            }
            out.push(')');
        }
    }
}

/// Encode a field list, sorted by name, as `name:type;name:type;...`.
fn encode_fields(fields: &[FieldSchema], out: &mut String) {
    let mut sorted: Vec<&FieldSchema> = fields.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, f) in sorted.iter().enumerate() {
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
    for vm in vms {
        out.push_str("VM:");
        out.push_str(&vm.name);
        out.push('{');
        encode_fields(&vm.fields, &mut out);
        out.push('}');
    }

    let mut cmds: Vec<&_> = m.commands.iter().collect();
    cmds.sort_by(|a, b| a.name.cmp(&b.name));
    for cmd in cmds {
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
pub fn contract_hash(m: &Manifest) -> String {
    let canonical = canonical_encoding(m);
    let digest = Sha256::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
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
}
