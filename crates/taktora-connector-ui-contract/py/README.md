# Python smoke proof (language-neutral contract)

`smoke.py` proves the MVVM UI connector's contract is genuinely
language-neutral from Python, using **only the standard library**. It validates
the in-repo golden manifest shipped with this crate
(`../tests/golden_manifest.json`) — the same fixture the Rust `tests/golden.rs`
test pins — so the cross-language reproducibility claim is checked in CI from
both Rust and Python against one canonical artefact.

## What it does (runnable)

```sh
python3 smoke.py                 # validates the in-repo golden manifest
python3 smoke.py path/to/manifest.json
```

It parses a JSON `Manifest`, recomputes the structural `contract_hash` in pure
Python via the canonical algorithm documented in
[`../CONTRACT.md`](../CONTRACT.md), and asserts it matches the `contract_hash`
the manifest carries. Exit code `0` means Python reproduced the hash
bit-for-bit — i.e. a non-Rust client can validate the contract it was built
against.

## The canonical hash algorithm (what Python reproduces)

The hash is `lowercase_hex(SHA-256(canonical_utf8))`. `canonical_utf8` is built
from the manifest **structure only** — `instance`, `epoch`, `contract_hash`, and
all service names are excluded. ViewModels, commands, fields and params are
sorted by name; enum variants by `(discriminant, name)`. Grammar:

```
manifest   := <each VM, sorted by name> <each command, sorted by name>
VM         := "VM:" name "{" field(";"field)* "}"
command    := "CMD:" name "(" param(";"param)* ")kind=" kind "idem=" bool
field/param:= name ":" fieldtype
fieldtype  := bool|i8..u64|f32|f64
            | "array<" fieldtype ";" len ">"
            | "str<" cap ">"
            | "struct{" field(";"field)* "}"
            | "enum:" name "<" width ">(" varname "=" disc ("," ...)* ")"
```

`kind` is the lowercase wire tag (`property`/`command`/`can_execute`/`event`),
`bool` is `true`/`false`. All names match `^[A-Za-z0-9_]+$` (none of the
delimiters), so the encoding is unambiguous.

## A live iceoryx2 Python client (how it would bind)

A live Python View binds dynamically off this same manifest — it never
constructs service names by convention. The flow:

1. **Discover & bind** — open a subscriber on the bootstrap manifest service
   `"<instance>.manifest"` (history depth 1), read the JSON `Manifest`, and
   validate `contract_hash(manifest)` against the hash it was generated for. On
   mismatch, fall back to read-only (commands disabled).
2. **Read a ViewModel** — resolve a ViewModel's service name from the manifest
   (`view_models[].service`), subscribe, and decode its JSON payload on each
   sample (e.g. the `System` heartbeat `{ "counter": u64, "epoch": u64 }`).
3. **Invoke a command** — mint a 32-byte correlation id, write the params JSON
   to the command's `request_service` carrying that correlation id, then read
   the `Ack` (`{"ack":"accepted"}` or
   `{"ack":"rejected","code":...,"message":...}`) off its `reply_service`,
   matching the correlation id.

Every UI service carries JSON inside a `ConnectorEnvelope`; the byte layout and
the full discovery/command contract are specified in
[`../CONTRACT.md`](../CONTRACT.md). The runnable, runtime-independent proof of
language-neutrality is the hash reproduction above.
