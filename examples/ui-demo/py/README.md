# Python smoke consumer (language-neutral contract)

`smoke.py` proves the MVVM UI connector's contract is genuinely
language-neutral from Python, using **only the standard library**.

## What it does (runnable)

```sh
python3 smoke.py                 # validates the checked-in golden manifest
python3 smoke.py path/to/manifest.json
```

It parses a JSON `Manifest`, recomputes the structural `contract_hash` in pure
Python via the canonical algorithm documented in
[`../../../crates/taktora-connector-ui-contract/CONTRACT.md`](../../../crates/taktora-connector-ui-contract/CONTRACT.md),
and asserts it matches the `contract_hash` the manifest carries. Exit code `0`
means Python reproduced the hash bit-for-bit — i.e. a non-Rust client can
validate the contract it was built against.

This is the **documented fallback** sanctioned by
`spec/requirements/connector/ui.rst`: standing up the iceoryx2 Python binding is
impractical in most environments, so the runnable, CI-friendly proof is the hash
reproduction (mirrored by the Rust test `tests/contract_reproducible.rs`).

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

## A live Python client (best-effort, optional)

```sh
python3 smoke.py --live          # uses the iceoryx2 binding if installed; else falls back
```

If the optional `iceoryx2` Python binding is available, a real client would:

1. **Discover & bind** — open a subscriber on the bootstrap manifest service
   `"<instance>.manifest"` (history depth 1), read the JSON `Manifest`, and
   validate `contract_hash(manifest)` against the hash it was generated for. On
   mismatch, fall back to read-only (commands disabled).
2. **Read the heartbeat** — resolve the `System` ViewModel's service name from
   the manifest (`view_models[].service`, e.g. `ui-demo/vm/System`), subscribe,
   and decode the JSON `{ "counter": u64, "epoch": u64 }` on each sample.
3. **Read a property** — same, for `Stepper`: `{ position, state, can_jog }`.
4. **Invoke a command** — mint a 32-byte correlation id, write the params JSON
   (e.g. `{"force":true}`) to the command's `request_service` carrying that
   correlation id, then read the `Ack` (`{"ack":"accepted"}` or
   `{"ack":"rejected","code":...,"message":...}`) off its `reply_service`,
   matching the correlation id.

### Wire envelope

Every UI service carries JSON inside a `ConnectorEnvelope<4096>`:

```
#[repr(C)] ZeroCopySend
sequence_number: u64
timestamp_ns:    u64
correlation_id:  [u8; 32]
payload_len:     u32
reserved:        u32
payload:         [u8; 4096]   # the UTF-8 JSON, first payload_len bytes valid
```

A Python binding decodes `payload[..payload_len]` as UTF-8 JSON. Service names
come from the manifest (never constructed by convention), except the bootstrap
`"<instance>.manifest"` name used to find the manifest itself.
