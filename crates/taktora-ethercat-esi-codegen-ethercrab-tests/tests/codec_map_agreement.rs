//! Cross-layer agreement: netcfg PDO resolver vs generated EL7047 codec.
//!
//! `taktora-ethercat-esi::EsiDevice::resolve_assignment` drives the netcfg
//! PDO_MAP; the generated `EL7047`/`EL7047OpMode` codec is what the driver
//! actually uses. They are derived from the same fixture but via two
//! independent code paths. This test pins the invariant that both paths
//! agree on the "Positioning interface" PDO assignment for the real EL7047.
//!
//! Guarded values (from ESI fixture `esi/beckhoff_el7047.xml`):
//!   rx indices : [0x1601, 0x1602, 0x1606]  (RxPDO = outputs)
//!   tx indices : [0x1A01, 0x1A03, 0x1A07]  (TxPDO = inputs)
//!   rx bits    : 176  → 22 output bytes
//!   tx bits    : 192  → 24 input  bytes

use taktora_ethercat_esi_codegen_ethercrab_tests::generated::{EL7047, EL7047OpMode};
use taktora_ethercat_esi_rt::EsiDevice;

/// Resolver (netcfg path) and generated codec agree on PDO indices and process-
/// image sizes for the "Positioning interface" mode of the real EL7047.
#[test]
fn positioning_interface_resolver_matches_generated_codec() {
    // ── Layer 1: parse + resolve via the ESI library (netcfg path) ──────────
    let xml = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/esi/beckhoff_el7047.xml"
    ));
    let esi = taktora_ethercat_esi::parse(xml).expect("parse EL7047 ESI fixture");

    // The fixture contains exactly one device; select it.
    let dev = esi
        .devices
        .iter()
        .find(|d| d.name.as_deref().unwrap_or("").contains("EL7047"))
        .expect("EL7047 device not found in fixture");

    let asg = dev
        .resolve_assignment(Some("Positioning interface"))
        .expect("resolve 'Positioning interface' assignment");

    // ── Layer 2: generated device codec ─────────────────────────────────────
    let codec = EL7047 {
        mode: EL7047OpMode::PositioningInterface(Default::default()),
    };
    let pa = codec.pdo_assignment();

    // ── Agreement: PDO index lists must match exactly ────────────────────────
    let resolver_rx: Vec<u16> = asg.rx.iter().map(|e| e.index).collect();
    let resolver_tx: Vec<u16> = asg.tx.iter().map(|e| e.index).collect();

    assert_eq!(
        resolver_rx, pa.rx,
        "rx PDO indices: resolver={resolver_rx:?} vs codec={:?}",
        pa.rx
    );
    assert_eq!(
        resolver_tx, pa.tx,
        "tx PDO indices: resolver={resolver_tx:?} vs codec={:?}",
        pa.tx
    );

    // ── Agreement: process-image sizes (bits == bytes * 8) ──────────────────
    let resolver_rx_bits: u32 = asg.rx.iter().map(|e| u32::from(e.bit_length)).sum();
    let resolver_tx_bits: u32 = asg.tx.iter().map(|e| u32::from(e.bit_length)).sum();

    assert_eq!(
        resolver_rx_bits,
        u32::try_from(codec.output_len()).unwrap() * 8,
        "rx (output) image size mismatch: resolver={resolver_rx_bits} bits vs codec={} bytes",
        codec.output_len()
    );
    assert_eq!(
        resolver_tx_bits,
        u32::try_from(codec.input_len()).unwrap() * 8,
        "tx (input) image size mismatch: resolver={resolver_tx_bits} bits vs codec={} bytes",
        codec.input_len()
    );

    // ── Pin absolute expected values (documents the golden truth) ───────────
    assert_eq!(
        resolver_rx,
        [0x1601u16, 0x1602u16, 0x1606u16],
        "rx PDO indices do not match expected golden values"
    );
    assert_eq!(
        resolver_tx,
        [0x1A01u16, 0x1A03u16, 0x1A07u16],
        "tx PDO indices do not match expected golden values"
    );
    assert_eq!(
        resolver_rx_bits, 176,
        "rx (output) image must be 176 bits = 22 bytes"
    );
    assert_eq!(
        resolver_tx_bits, 192,
        "tx (input) image must be 192 bits = 24 bytes"
    );
}
