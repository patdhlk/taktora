//! Codegen emits the resolved SM watchdog for output devices and nothing
//! for input-only devices (`REQ_0844`, `TEST_0860` codegen half).
//!
//! An rx-carrying device's `PDO_MAP` entry is
//! `SubDeviceMap::new(..).with_sm_watchdog(SmWatchdog { divider, intervals })`;
//! an input-only device's entry is a bare `SubDeviceMap::new(..)` with no
//! `with_sm_watchdog` chain.

/// A two-device bus: an input-only `din` (tx only) and an output `dout`
/// (rx, attested). Default FTTI/2 = 50 ms → 500 ticks at divider 2498.
const FIXTURE: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: din,  pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: dout, sm_watchdog_enabled: true, pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels: []
";

/// Codegen output as a formatted source string.
fn generate(yaml: &str) -> String {
    let cfg = taktora_ethercat_netcfg::parse(yaml).expect("fixture parses");
    taktora_ethercat_netcfg_codegen::generate(&cfg).expect("codegen succeeds")
}

/// The ordered `PDO_MAP` element expressions parsed from generated source.
fn pdo_map_elems(file: &syn::File) -> Vec<&syn::Expr> {
    let pdo_map = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Static(s) if s.ident == "PDO_MAP" => Some(s),
            _ => None,
        })
        .expect("generated source defines a `static PDO_MAP`");
    match &*pdo_map.expr {
        syn::Expr::Reference(r) => match &*r.expr {
            syn::Expr::Array(a) => a.elems.iter().collect(),
            _ => panic!("PDO_MAP initializer is not an array reference"),
        },
        _ => panic!("PDO_MAP initializer is not a reference"),
    }
}

#[test]
fn output_device_emits_with_sm_watchdog() {
    let src = generate(FIXTURE);
    let file = syn::parse_file(&src).expect("generated source is valid Rust");
    let elems = pdo_map_elems(&file);
    assert_eq!(elems.len(), 2, "two devices → two PDO_MAP entries");

    // din (index 0, input-only) emits a bare `new(..)` call — no method chain.
    assert!(
        matches!(elems[0], syn::Expr::Call(_)),
        "input-only device emits a bare SubDeviceMap::new(..), no watchdog:\n{src}"
    );

    // dout (index 1, output) emits `new(..).with_sm_watchdog(..)`.
    let syn::Expr::MethodCall(mc) = elems[1] else {
        panic!("output device must emit a `.with_sm_watchdog(..)` method call:\n{src}");
    };
    assert_eq!(
        mc.method, "with_sm_watchdog",
        "the trailing method call is with_sm_watchdog"
    );
    assert!(
        matches!(&*mc.receiver, syn::Expr::Call(_)),
        "with_sm_watchdog is chained on the SubDeviceMap::new(..) call"
    );

    // The emitted source carries the resolved register values
    // (divider 2498, intervals 500 for the default-FTTI/2 50 ms window) and
    // names the connector `SmWatchdog` type textually.
    assert!(
        src.contains("with_sm_watchdog"),
        "source mentions with_sm_watchdog:\n{src}"
    );
    assert!(
        src.contains("2498") && src.contains("500"),
        "source carries divider 2498 and intervals 500:\n{src}"
    );
    assert!(
        src.contains("SmWatchdog"),
        "source names the connector SmWatchdog type:\n{src}"
    );
}

#[test]
fn override_value_is_emitted() {
    // 10 ms override → 100 ticks at divider 2498.
    let yaml = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: dout, sm_watchdog_enabled: true, sm_watchdog_timeout_ms: 10, pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels: []
";
    let src = generate(yaml);
    assert!(
        src.contains("2498") && src.contains("100"),
        "10 ms override emits divider 2498, intervals 100:\n{src}"
    );
}
