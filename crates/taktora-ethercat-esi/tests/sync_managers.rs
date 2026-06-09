//! `REQ_0504` — sync managers in the IR.
use taktora_ethercat_esi::{SmDirection, parse};

const EL3001: &str = include_str!("fixtures/el3001_like.xml");

#[test]
fn parses_sync_managers_in_order() {
    let file = parse(EL3001).expect("fixture parses");
    let sms = &file.devices[0].sync_managers;
    assert_eq!(sms.len(), 3);

    assert_eq!(sms[0].index, 0);
    assert_eq!(sms[0].start_address, 0x1000);
    assert_eq!(sms[0].control_byte, 0x26);
    assert!(sms[0].enable);
    // Positive Output-direction check: #x26 dir-bits = 0b01 = Output (master
    // writes / MBoxOut). Guards against re-inverting the decode (this exact
    // bug was latent because only the Input side was asserted).
    assert_eq!(sms[0].direction, SmDirection::Output);

    assert_eq!(sms[2].start_address, 0x1100);
    assert_eq!(sms[2].control_byte, 0x00);
    assert_eq!(sms[2].direction, SmDirection::Input);
}
