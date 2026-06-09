//! `AlternativeSmMapping` parsing (issue #70).

use taktora_ethercat_esi::parse;

const FIXTURE: &str = include_str!("fixtures/el7047_alt_sm.xml");

#[test]
fn parses_alternative_sm_mappings_in_order() {
    let file = parse(FIXTURE).expect("fixture parses");
    let dev = &file.devices[0];
    assert_eq!(dev.alt_sm_mappings.len(), 2);

    let m0 = &dev.alt_sm_mappings[0];
    assert_eq!(m0.name.as_deref(), Some("Velocity control compact"));
    assert!(m0.default, "first mapping carries Default=1");
    assert_eq!(m0.sm_assignments.len(), 2);
    assert_eq!(m0.sm_assignments[0].sm, 2);
    assert_eq!(m0.sm_assignments[0].pdos[0].index, 0x1600);
    assert_eq!(m0.sm_assignments[0].pdos[0].channel_no, Some(1));
    assert_eq!(m0.sm_assignments[1].sm, 3);
    assert_eq!(m0.sm_assignments[1].pdos[0].index, 0x1a00);
    assert_eq!(m0.sm_assignments[1].pdos[0].channel_no, Some(1));

    let m1 = &dev.alt_sm_mappings[1];
    assert_eq!(m1.name.as_deref(), Some("Positioning interface"));
    assert!(!m1.default);
    assert_eq!(m1.sm_assignments.len(), 2);
    assert_eq!(m1.sm_assignments[0].pdos[0].index, 0x1601);
    assert_eq!(m1.sm_assignments[0].pdos[0].channel_no, Some(1));
    assert_eq!(m1.sm_assignments[0].pdos[1].index, 0x1611);
    assert_eq!(m1.sm_assignments[0].pdos[1].channel_no, None);
    assert_eq!(m1.sm_assignments[1].pdos[0].index, 0x1a01);
    assert_eq!(m1.sm_assignments[1].pdos[0].channel_no, Some(1));
}

#[test]
fn device_without_vendor_block_has_no_mappings() {
    // EL1008 fixture has no AlternativeSmMapping.
    let el1008 = include_str!("fixtures/beckhoff_el1008.xml");
    let file = parse(el1008).expect("el1008 parses");
    assert!(file.devices[0].alt_sm_mappings.is_empty());
}
