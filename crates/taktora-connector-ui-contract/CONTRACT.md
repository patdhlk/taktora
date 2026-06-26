# UI connector wire contract

This crate defines the **language-neutral MVVM contract** for the taktora UI
connector (FEAT_0092). Its JSON serialization *is* the cross-language wire
contract: any UI process — Rust, Python, or otherwise — binds dynamically by
reading the published [manifest](#manifest) and validating its
[`contract_hash`](#contract-hash). Nothing here depends on iceoryx2, the
executor, or any UI framework.

The canonical example is checked in at
[`tests/golden_manifest.json`](tests/golden_manifest.json) and asserted by the
`golden` integration test. Regenerate it after an intentional change with:

```sh
cargo test -p taktora-connector-ui-contract --test golden -- --ignored regenerate
```

## Manifest

The top-level object a UI binds against. It is the **sole source of service
names** (REQ_0873).

| Field           | JSON type | Notes                                                        |
| --------------- | --------- | ----------------------------------------------------------- |
| `instance`      | string    | The instance namespace prefixing every service name.        |
| `epoch`         | number    | Process-unique epoch for this connector incarnation.        |
| `contract_hash` | string    | Lowercase-hex SHA-256 structural hash (see below).          |
| `view_models`   | array     | The published [ViewModels](#viewmodel).                     |
| `commands`      | array     | The available [commands](#command).                         |

### ViewModel

A fixed-layout POD struct published latest-value (history depth 1) over one
service.

| Field     | JSON type | Notes                                            |
| --------- | --------- | ------------------------------------------------ |
| `name`    | string    | Logical name.                                    |
| `service` | string    | Fully-qualified, instance-namespaced service.    |
| `fields`  | array     | [Field schemas](#field-schema), declaration order. |

### Command

An acceptance-acked request/response action.

| Field                 | JSON type | Notes                                                   |
| --------------------- | --------- | ------------------------------------------------------- |
| `name`                | string    | Logical name.                                           |
| `request_service`     | string    | Carries invocation requests.                            |
| `reply_service`       | string    | Carries acceptance acks.                                |
| `params`              | array     | [Field schemas](#field-schema), declaration order.      |
| `kind`                | string    | The entry [kind](#kind) (`command`).                    |
| `idempotent`          | bool      | Whether safe to auto-retry under the same correlation.  |
| `can_execute_service` | string?   | Optional CanExecute gate service; omitted when absent.  |

## Kind

Stable `snake_case` tags: `property`, `command`, `can_execute`, and the reserved
`event` (the deferred lossless event stream is not yet emitted).

## Field schema

A `{ "name": <string>, "type": <FieldType> }` pair. `FieldType` is internally
tagged on `"type"` with `snake_case` tags from a **closed POD set**:

| Tag     | Extra keys             | Meaning                                                  |
| ------- | ---------------------- | ------------------------------------------------------- |
| `bool`  | —                      | Boolean.                                                 |
| `i8`…`i64`, `u8`…`u64` | —       | Signed / unsigned integers of the named width.          |
| `f32`, `f64` | —                 | IEEE-754 floats.                                         |
| `array` | `elem`, `len`          | Fixed-length array of `len` `elem`s.                     |
| `str`   | `cap`                  | Inline bounded UTF-8: a `len: u16` then `[u8; cap]`.     |
| `struct`| `fields`               | Nested POD struct.                                       |
| `enum`  | `name`, `variants`, `width` | C-like enum lowered to a `width`-byte backing int. `variants` is an array of `[name, discriminant]` pairs. |

Types outside this set (`Vec`, `String`, `HashMap`, `i128`/`u128`, …) are
rejected at authoring time (REQ_0858).

## Command ack

The reply on a command's `reply_service`, adjacently tagged on `"ack"`:

- `{ "ack": "accepted" }` — accepted for execution (at-most-once).
- `{ "ack": "rejected", "code": <RejectedCode>, "message": <string> }` — the
  effect did not run.

`RejectedCode` is a closed `snake_case` set: `can_execute_false`,
`invalid_args`, `faulted`, `back_pressure`, `unknown_command`,
`contract_mismatch`.

## Contract hash

`contract_hash = lowercase_hex(SHA-256(canonical_utf8))`. The canonical encoding
is built from the manifest **structure only** — `instance`, `epoch`,
`contract_hash`, and *all* service names are excluded (they depend on the
instance namespace and must not affect compatibility). Any other language MUST
reproduce byte-identical `canonical_utf8`.

Grammar (ViewModels and commands sorted by `name`; fields and params sorted by
`name`; enum variants sorted by `(discriminant, name)`; UTF-8 byte order
throughout):

```text
manifest   := viewmodels commands
viewmodels := ( "VM:" name "{" field (";" field)* "}" )*
commands   := ( "CMD:" name "(" param (";" param)* ")" "kind=" kind "idem=" bool )*
field      := name ":" fieldtype
param      := name ":" fieldtype
fieldtype  := "bool" | "i8" | "i16" | "i32" | "i64"
            | "u8" | "u16" | "u32" | "u64" | "f32" | "f64"
            | "array<" fieldtype ";" len ">"
            | "str<" cap ">"
            | "struct{" field (";" field)* "}"
            | "enum:" name "<" width ">(" varname "=" disc ("," varname "=" disc)* ")"
kind       := "property" | "command" | "can_execute" | "event"
bool       := "true" | "false"
```

The reference implementation is [`hash::contract_hash`](src/hash.rs).
