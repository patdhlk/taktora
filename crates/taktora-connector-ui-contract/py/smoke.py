#!/usr/bin/env python3
"""Language-neutral smoke proof for the taktora MVVM UI connector (FEAT_0092).

This script demonstrates that the UI connector's contract is genuinely
language-neutral: a non-Rust consumer can recompute the structural
``contract_hash`` from the published JSON manifest using the canonical algorithm
documented in ``crates/taktora-connector-ui-contract/CONTRACT.md``.

Given a manifest JSON file (defaults to the in-repo golden manifest shipped
with this crate at ``tests/golden_manifest.json``), recompute the contract hash
in pure Python (standard library only) and assert it matches the
``contract_hash`` the manifest carries. This is the runnable proof of
cross-language reproducibility, and it pins exactly the same golden fixture the
Rust ``tests/golden.rs`` test pins.

    python3 smoke.py            # validates the in-repo golden manifest
    python3 smoke.py <manifest.json>

A live iceoryx2 Python client would bind dynamically off this same manifest
(discover services, validate the hash, subscribe to ViewModels, invoke
commands); see ``../CONTRACT.md`` for the wire envelope and the discovery flow.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

# The golden manifest fixture shipped with this contract crate; the canonical
# wire example every language targets.
GOLDEN = Path(__file__).resolve().parents[1] / "tests" / "golden_manifest.json"

# Scalar FieldType tags encode to themselves.
_SCALARS = {
    "bool",
    "i8",
    "i16",
    "i32",
    "i64",
    "u8",
    "u16",
    "u32",
    "u64",
    "f32",
    "f64",
}


def encode_field_type(ty: dict) -> str:
    """Encode a FieldType object to its canonical string (see CONTRACT.md)."""
    tag = ty["type"]
    if tag in _SCALARS:
        return tag
    if tag == "array":
        return f"array<{encode_field_type(ty['elem'])};{ty['len']}>"
    if tag == "str":
        return f"str<{ty['cap']}>"
    if tag == "struct":
        return "struct{" + encode_fields(ty["fields"]) + "}"
    if tag == "enum":
        # Variants sorted by (discriminant, name).
        variants = sorted(ty["variants"], key=lambda v: (v[1], v[0]))
        body = ",".join(f"{name}={disc}" for name, disc in variants)
        return f"enum:{ty['name']}<{ty['width']}>({body})"
    raise ValueError(f"unknown field type tag: {tag!r}")


def encode_fields(fields: list[dict]) -> str:
    """Encode a field list, sorted by name, as ``name:type;name:type;...``."""
    ordered = sorted(fields, key=lambda f: f["name"])
    return ";".join(f"{f['name']}:{encode_field_type(f['type'])}" for f in ordered)


def canonical_encoding(manifest: dict) -> str:
    """Build the canonical UTF-8 structural encoding of a manifest.

    Excludes ``instance``, ``epoch``, ``contract_hash`` and every service name.
    """
    out: list[str] = []

    for vm in sorted(manifest["view_models"], key=lambda v: v["name"]):
        out.append("VM:")
        out.append(vm["name"])
        out.append("{")
        out.append(encode_fields(vm["fields"]))
        out.append("}")

    for cmd in sorted(manifest["commands"], key=lambda c: c["name"]):
        out.append("CMD:")
        out.append(cmd["name"])
        out.append("(")
        out.append(encode_fields(cmd["params"]))
        out.append(")kind=")
        out.append(cmd["kind"])
        out.append("idem=")
        out.append("true" if cmd["idempotent"] else "false")

    return "".join(out)


def contract_hash(manifest: dict) -> str:
    """Lowercase-hex SHA-256 of the canonical encoding (REQ_0874)."""
    return hashlib.sha256(canonical_encoding(manifest).encode("utf-8")).hexdigest()


def run_hash_check(path: Path) -> int:
    manifest = json.loads(path.read_text())
    recomputed = contract_hash(manifest)
    embedded = manifest.get("contract_hash", "")
    print(f"manifest:    {path}")
    print(f"instance:    {manifest.get('instance')!r}  epoch={manifest.get('epoch')}")
    print(f"canonical:   {canonical_encoding(manifest)}")
    print(f"recomputed:  {recomputed}")
    print(f"embedded:    {embedded}")
    if recomputed == embedded:
        print("OK: Python reproduced the contract hash from the JSON manifest.")
        return 0
    print("FAIL: hash mismatch — the contract is not reproducible.", file=sys.stderr)
    return 1


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith("-")]
    path = Path(args[0]) if args else GOLDEN
    return run_hash_check(path)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
