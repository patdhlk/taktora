# ESI fixtures

Trimmed Beckhoff ESI files fed to `taktora-ethercat-esi-build` at build
time (`build.rs` globs `esi/*.xml`).

- **`beckhoff_el7047.xml`** — a trimmed single-device excerpt (EL7047,
  ProductCode `#x1b873052`, RevisionNo `#x00170000`) of the Beckhoff
  EL70xx vendor ESI, transcoded from Windows-1252 to UTF-8. The full
  vendor catalog is intentionally **not committed** (size and
  licensing); `.gitignore` keeps any dropped-in full files local-only,
  allow-listing only the three trimmed fixtures.
- **`beckhoff_el1008.xml`**, **`beckhoff_el2004.xml`** — the same
  trimmed fixtures used by `examples/ethercat-real-bus`.

## Codegen caveat: the EL7047 codec is hand-written

The EL7047 typed PDO codec is **hand-written in `src/el7047.rs`, not
codegen-generated**. The ESI codegen treats Beckhoff `Fixed="1"` PDOs as
always-on and emits the *union* of all 19 of the EL7047's PDOs — which
matches no single SyncManager assignment. It cannot model this device's
selectable "Positioning interface" assignment (Rx `0x1601`+`0x1602`+
`0x1606`, Tx `0x1a01`+`0x1a03`+`0x1a07`). So codegen still runs over
`beckhoff_el7047.xml` and produces an `EL7047` type, but the example
**ignores it** and uses the hand-written 22-byte/24-byte image codec
instead. That codegen `Fixed`-vs-assignment limitation is tracked
separately and is not fixed here.

**EL1008 and EL2004 do use codegen** — each has a single fixed PDO
assignment the codegen models correctly, so `EL1008::decode_inputs` and
`EL2004::encode_outputs` are used as-is (as in `ethercat-real-bus`).
