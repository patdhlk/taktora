#!/usr/bin/env python3
"""Language-neutral smoke consumer for the taktora MVVM UI connector (FEAT_0092).

This script demonstrates that the UI connector's contract is genuinely
language-neutral: a non-Rust consumer can both (a) recompute the structural
``contract_hash`` from the published JSON manifest using the canonical algorithm
documented in ``crates/taktora-connector-ui-contract/CONTRACT.md``, and (b)
read the live ``System`` heartbeat off the JSON pub/sub plane.

Two modes:

1. **Hash reproduction (always runnable, pure stdlib).** Given a manifest JSON
   file (defaults to the checked-in golden fixture), recompute the contract hash
   in pure Python and assert it matches the ``contract_hash`` the manifest
   carries. This is the runnable proof of cross-language reproducibility and is
   the path the example's CI/test relies on.

       python3 smoke.py            # validates the golden manifest
       python3 smoke.py <manifest.json>

2. **Live read (best-effort).** If the optional ``iceoryx2`` Python binding is
   installed, connect to a running ``ui-demo`` producer, read the manifest off
   ``ui-demo.manifest`` and the heartbeat off the ``System`` ViewModel service.
   The binding is intentionally optional: standing up the iceoryx2 Python
   binding is impractical in most environments, and ``spec/requirements/connector/ui.rst``
   sanctions this documented fallback. See ``README.md`` for the wire details a
   real Python client would bind against.

       python3 smoke.py --live
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

# The golden manifest fixture shipped with the contract crate; the canonical
# wire example every language targets.
GOLDEN = (
    Path(__file__).resolve().parents[3]
    / "crates"
    / "taktora-connector-ui-contract"
    / "tests"
    / "golden_manifest.json"
)

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


def run_live() -> int:
    try:
        import iceoryx2  # noqa: F401  (optional binding)
    except ImportError:
        print(
            "iceoryx2 Python binding not installed — see README.md for how a "
            "live Python client would subscribe to 'ui-demo.manifest' and the "
            "'ui-demo/vm/System' heartbeat service. Falling back to the hash "
            "check, which is the sanctioned, runnable proof of language-neutrality.",
            file=sys.stderr,
        )
        return run_hash_check(GOLDEN)

    # If the binding IS present, a real client would:
    #   1. open a subscriber on "ui-demo.manifest" (history depth 1), read the
    #      JSON Manifest, and validate contract_hash == contract_hash(manifest);
    #   2. resolve the System service name from the manifest, subscribe, and
    #      decode the JSON {counter, epoch} heartbeat each sample.
    # The envelope is ConnectorEnvelope<4096>: a fixed [u8;4096] payload with a
    # u32 payload_len prefix region (see README.md for the layout).
    print(
        "iceoryx2 binding present, but the live-read path is left as a documented "
        "stub for this example. See README.md.",
        file=sys.stderr,
    )
    return run_hash_check(GOLDEN)


def main(argv: list[str]) -> int:
    if "--live" in argv:
        return run_live()
    args = [a for a in argv if not a.startswith("-")]
    path = Path(args[0]) if args else GOLDEN
    return run_hash_check(path)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
