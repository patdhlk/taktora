# Vendor ESI fixtures

Real, publicly-downloadable vendor ESI XML files used by `vendor_real.rs`
(TEST_0400 — "parse() accepts a representative Beckhoff EL3001 ESI").

These files are vendor copyright; they are NOT redistributed as part of the
crate's published package (see `Cargo.toml` `exclude`). To run the real-file
tests, drop the files below into this directory.

Do NOT commit the ETG Conformance Test Tool (CTT) test file set — it is
members-only, Vendor-ID-gated, and not redistributable.

| File | Device | Source URL | Retrieved | Notes |
|------|--------|------------|-----------|-------|
| `Beckhoff_EL3001.xml` | EL3001 1Ch analog input | https://www.beckhoff.com/ (Download -> ESI) | TODO | TODO |

## Expected characteristics asserted by `vendor_real.rs`
- Vendor id `0x00000002` (Beckhoff)
- At least one device whose product name contains `EL3001`
- That device has >=1 TxPDO with >=1 entry, and a CoE mailbox.
