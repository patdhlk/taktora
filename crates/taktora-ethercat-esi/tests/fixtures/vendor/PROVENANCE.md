# Vendor ESI fixtures

Real, publicly-downloadable vendor ESI XML files used by the real-vendor
integration tests: `vendor_real.rs` (TEST_0400 — "parse() accepts a
representative Beckhoff EL3001 ESI") and `wago_real.rs` (TEST_0867 —
"parse() accepts a real WAGO 750-354 modular-coupler ESI").

These files are vendor copyright; they are NOT redistributed as part of the
crate's published package (see `Cargo.toml` `exclude`). To run the real-file
tests, drop the files below into this directory.

Do NOT commit the ETG Conformance Test Tool (CTT) test file set — it is
members-only, Vendor-ID-gated, and not redistributable.

| File | Device | Source URL | Retrieved | Notes |
|------|--------|------------|-----------|-------|
| `Beckhoff_EL3001.xml` | EL3001 1Ch analog input | https://www.beckhoff.com/ (Download -> ESI) | TODO | TODO |
| `WAGO_750-354_25.xml` | 750-354 EtherCAT fieldbus coupler | https://www.wago.com/ (Downloads -> ESI) | 2026-06-07 | Full MDP description (vendor file dated 2023-11): single-file module catalog (~270 modules) + `<Slots>`; exercises Modules/Slots (TEST_0867) |
| `WAGO_750_354_no_modules.xml` | 750-354 EtherCAT fieldbus coupler | https://www.wago.com/ (Downloads -> ESI) | 2026-06-07 | Module-less variant (vendor file dated 2019-03): coupler devices only, no `<Modules>` catalog (TEST_0867) |

## Expected characteristics asserted by `vendor_real.rs`
- Vendor id `0x00000002` (Beckhoff)
- At least one device whose product name contains `EL3001`
- That device has >=1 TxPDO with >=1 entry, and a CoE mailbox.

## Expected characteristics asserted by `wago_real.rs`
- Vendor id `0x00000021` (WAGO), both files
- A device whose name or type contains `750-354`, both files
- `WAGO_750-354_25.xml`: that device declares `<Slots>` with >=1 slot, the
  file carries a non-empty `<Modules>` catalog, and every slot-referenced
  `ModuleIdent` resolves against that catalog
- `WAGO_750_354_no_modules.xml`: the file carries NO module catalog
