//! SM-watchdog config-time validation matrix (`REQ_0845`, `TEST_0861`).
//!
//! For every device carrying output (rx) PDOs, `parse`/`resolve` enforces
//! two facts (AOU_0016): the QUANTIZED effective timeout must be ≤ FTTI/2,
//! and the watchdog must be ENABLED — attested via the ESI's output
//! process-data SM(s) (`DeviceSource::Esi`) or the explicit
//! `sm_watchdog_enabled` flag (`DeviceSource::Inline`). Input-only devices
//! are untouched by both checks.
//!
//! The doc comments spell out tick arithmetic as prose, so `doc_markdown`
//! is silenced file-wide.
#![allow(clippy::doc_markdown)]

use std::io::Write as _;

use taktora_ethercat_netcfg::{NetcfgError, parse};

/// Build a single-device ESI whose RxPDO output SM (index 0) carries the
/// given control byte, plus a mailbox-less input SM. `control_byte`:
/// `#x44` = Output direction + watchdog-trigger enabled; `#x04` = Output
/// direction + watchdog disabled.
fn esi_with_output_sm(output_sm_control_byte: &str) -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>#x00000021</Id></Vendor>
  <Descriptions><Devices><Device>
    <Type ProductCode="#x07500354" RevisionNo="#x00000001">Outputs</Type>
    <Name>Outputs</Name>
    <Sm StartAddress="#x1000" ControlByte="{output_sm_control_byte}" Enable="1">Outputs</Sm>
    <RxPdo Sm="0">
      <Index>#x1600</Index>
      <Entry><Index>#x7000</Index><BitLen>8</BitLen></Entry>
    </RxPdo>
  </Device></Devices></Descriptions>
</EtherCATInfo>"##
    )
}

/// Write an ESI string to a temp file and build a one-device network.yaml
/// referencing it, with an optional per-device timeout override and FTTI.
fn esi_yaml(
    esi_xml: &str,
    ftti_ms: Option<u64>,
    timeout_ms: Option<u64>,
) -> (tempfile::NamedTempFile, String) {
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(esi_xml.as_bytes())
        .expect("write ESI fixture");
    let esi_path = esi.path().to_str().expect("ESI path is UTF-8").to_owned();

    let ftti_line = ftti_ms.map_or(String::new(), |v| format!(", ftti_ms: {v}"));
    let timeout_line = timeout_ms.map_or(String::new(), |v| {
        format!("\n    sm_watchdog_timeout_ms: {v}")
    });
    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256{ftti_line} }}
devices:
  - label: outputs{timeout_line}
    esi: "{esi_path}"
channels: []
"#
    );
    (esi, yaml)
}

// ---- ESI-sourced enable attestation -------------------------------------

/// PASS — ESI output SM has the watchdog trigger enabled (`#x44`), default
/// FTTI/2 timeout well under the bound.
#[test]
fn esi_enabled_default_ftti_passes() {
    let (_esi, yaml) = esi_yaml(&esi_with_output_sm("#x44"), None, None);
    parse(&yaml).expect("ESI-enabled output device under FTTI/2 passes");
}

/// PASS — override (10 ms) below FTTI/2 (50 ms), ESI enabled.
#[test]
fn esi_enabled_override_below_bound_passes() {
    let (_esi, yaml) = esi_yaml(&esi_with_output_sm("#x44"), None, Some(10));
    parse(&yaml).expect("override below bound passes");
}

/// FAIL — override above FTTI/2 (60 ms > 50 ms). Error names the device,
/// the offending effective value, and the bound.
#[test]
fn override_above_bound_fails() {
    let (_esi, yaml) = esi_yaml(&esi_with_output_sm("#x44"), None, Some(60));
    let err = parse(&yaml).expect_err("60 ms > FTTI/2 (50 ms) must fail");
    assert!(
        matches!(err, NetcfgError::SmWatchdogTimeoutTooLong { ref label, .. } if label == "outputs"),
        "expected SmWatchdogTimeoutTooLong for `outputs`, got {err:?}"
    );
}

/// Validation compares the QUANTIZED effective timeout (ceil result), not
/// the raw request — so a request that is ≤ the bound can still fail once
/// quantization rounds it up past the bound. The public YAML knobs are
/// ms-granular (`ftti_ms`, `sm_watchdog_timeout_ms`), so an override is
/// always a whole number of ms = a whole number of 1000 µs = a whole
/// number of 100 µs ticks, and the bound (FTTI/2) is always a whole
/// number of 500 µs = a whole number of ticks. Both therefore land
/// exactly on the tick grid and quantization never overshoots through
/// YAML alone — the FTTI/2 boundary is exact. The sub-tick
/// quantization-over path (request below bound, quantized effective above
/// it) is exercised value-for-value against the connector semantics in
/// `sm_watchdog_resolution.rs::arithmetic_matches_connector_semantics`
/// (cf. the connector's `from_timeout_us(50_001)` → 501 ticks case). Here
/// we pin the exact-grid boundary: override == FTTI/2 == 500 ticks →
/// quantized effective == bound → PASS, proving the comparison is `<=`,
/// not `<`.
#[test]
fn quantized_boundary_is_inclusive() {
    // FTTI default 100 ms → bound 50 ms. Override 50 ms == bound exactly →
    // 500 ticks → effective 50 ms == bound → PASS.
    let (_esi, yaml) = esi_yaml(&esi_with_output_sm("#x44"), None, Some(50));
    parse(&yaml).expect("override exactly at FTTI/2 (on the tick grid) passes");
}

/// FAIL — one ms over the bound. FTTI 100 ms → bound 50 ms; override 51 ms
/// → 510 ticks → effective 51 ms > 50 ms. The quantized effective drives
/// the comparison.
#[test]
fn quantized_effective_one_ms_over_bound_fails() {
    let (_esi, yaml) = esi_yaml(&esi_with_output_sm("#x44"), None, Some(51));
    let err = parse(&yaml).expect_err("51 ms > FTTI/2 (50 ms) must fail");
    assert!(
        matches!(err, NetcfgError::SmWatchdogTimeoutTooLong { ref label, .. } if label == "outputs"),
        "expected SmWatchdogTimeoutTooLong for `outputs`, got {err:?}"
    );
}

/// FAIL — ESI says the output SM's watchdog trigger is DISABLED (`#x04`).
#[test]
fn esi_disabled_output_sm_fails() {
    let (_esi, yaml) = esi_yaml(&esi_with_output_sm("#x04"), None, None);
    let err = parse(&yaml).expect_err("ESI watchdog-disabled output SM must fail");
    assert!(
        matches!(err, NetcfgError::SmWatchdogDisabled { ref label } if label == "outputs"),
        "expected SmWatchdogDisabled for `outputs`, got {err:?}"
    );
}

/// Write an ESI string to a temp file and build a one-device network.yaml
/// referencing it, with an optional per-device timeout override, FTTI, and
/// operator attestation (`sm_watchdog_enabled`).
fn esi_yaml_attested(
    esi_xml: &str,
    sm_watchdog_enabled: Option<bool>,
) -> (tempfile::NamedTempFile, String) {
    let mut esi = tempfile::Builder::new()
        .suffix(".xml")
        .tempfile()
        .expect("create temp ESI file");
    esi.write_all(esi_xml.as_bytes())
        .expect("write ESI fixture");
    let esi_path = esi.path().to_str().expect("ESI path is UTF-8").to_owned();

    let attest_line =
        sm_watchdog_enabled.map_or(String::new(), |b| format!("\n    sm_watchdog_enabled: {b}"));
    let yaml = format!(
        r#"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - label: outputs{attest_line}
    esi: "{esi_path}"
channels: []
"#
    );
    (esi, yaml)
}

/// PASS — ESI output SM has the watchdog trigger DISABLED (`#x04`) but the
/// operator attests `sm_watchdog_enabled: true`, taking responsibility. The
/// connector programs `0x0400`/`0x0420` regardless; attestation is the
/// deliberate escape hatch for real devices (e.g. Beckhoff EL7047) whose ESI
/// does not set the trigger bit even though the watchdog operates at runtime.
#[test]
fn esi_disabled_sm_with_operator_attestation_passes() {
    let (_esi, yaml) = esi_yaml_attested(&esi_with_output_sm("#x04"), Some(true));
    let cfg = parse(&yaml).expect("ESI-disabled SM + operator attestation must pass");
    let dev = cfg.devices.iter().find(|d| d.label == "outputs").unwrap();
    assert!(
        dev.sm_watchdog.is_some(),
        "resolved sm_watchdog must be Some(..) when attestation opens the gate"
    );
}

/// FAIL — same ESI (watchdog trigger DISABLED, `#x04`) but NO operator
/// attestation. Must still error `SmWatchdogDisabled`; default-deny is
/// unchanged when neither ESI evidence nor explicit attestation is present.
#[test]
fn esi_disabled_sm_without_attestation_still_fails() {
    let (_esi, yaml) = esi_yaml_attested(&esi_with_output_sm("#x04"), None);
    let err = parse(&yaml).expect_err("ESI-disabled SM without attestation must still fail");
    assert!(
        matches!(err, NetcfgError::SmWatchdogDisabled { ref label } if label == "outputs"),
        "expected SmWatchdogDisabled for `outputs`, got {err:?}"
    );
}

// ---- Inline-sourced enable attestation ----------------------------------

/// Build an inline-source one-device network.yaml with rx PDOs and an
/// optional `sm_watchdog_enabled` attestation.
fn inline_yaml(enabled: Option<bool>) -> String {
    let attest = enabled.map_or(String::new(), |b| format!("\n    sm_watchdog_enabled: {b}"));
    format!(
        r"
schema_version: 1
bus: {{ cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }}
devices:
  - label: outputs{attest}
    pdos:
      rx: [{{ index: 0x7000, bit_offset: 0, bit_length: 8 }}]
channels: []
"
    )
}

/// FAIL — Inline outputs with no `sm_watchdog_enabled` attestation.
#[test]
fn inline_outputs_without_attestation_fails() {
    let err = parse(&inline_yaml(None)).expect_err("inline outputs without attestation must fail");
    assert!(
        matches!(err, NetcfgError::SmWatchdogNotAttested { ref label } if label == "outputs"),
        "expected SmWatchdogNotAttested for `outputs`, got {err:?}"
    );
}

/// FAIL — Inline outputs explicitly attest `sm_watchdog_enabled: false`.
#[test]
fn inline_outputs_attested_false_fails() {
    let err = parse(&inline_yaml(Some(false))).expect_err("attested-false must fail");
    assert!(
        matches!(err, NetcfgError::SmWatchdogNotAttested { ref label } if label == "outputs"),
        "expected SmWatchdogNotAttested for `outputs`, got {err:?}"
    );
}

/// PASS — Inline outputs with `sm_watchdog_enabled: true`.
#[test]
fn inline_outputs_attested_true_passes() {
    parse(&inline_yaml(Some(true))).expect("attested-true inline outputs pass");
}

// ---- Input-only devices are untouched -----------------------------------

/// An input-only inline device (tx only) needs no attestation and is never
/// validated for the watchdog bound.
#[test]
fn input_only_device_skipped() {
    let yaml = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
devices:
  - label: inputs
    pdos:
      tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }]
channels: []
";
    parse(yaml).expect("input-only device is skipped by watchdog validation");
}
