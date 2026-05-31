//! `REQ_0504` — mailbox (CoE/EoE/FoE/SoE support + `InitCmds`).
use taktora_ethercat_esi::{Transition, parse};

const EL3001: &str = include_str!("fixtures/el3001_like.xml");
const INITCMDS: &str = include_str!("fixtures/mailbox_initcmds.xml");

#[test]
fn parses_coe_support_flags() {
    let file = parse(EL3001).expect("fixture parses");
    let mb = file.devices[0]
        .mailbox
        .as_ref()
        .expect("device has a mailbox");
    let coe = mb.coe.as_ref().expect("CoE declared");
    assert!(coe.sdo_info);
    assert!(!coe.pdo_assign);
    assert!(!mb.foe);
    assert!(!mb.eoe);
}

#[test]
fn parses_init_cmds_with_transition_and_payload() {
    let file = parse(INITCMDS).expect("fixture parses");
    let mb = file.devices[0].mailbox.as_ref().expect("mailbox");
    assert_eq!(mb.init_cmds.len(), 1);
    let cmd = &mb.init_cmds[0];
    assert_eq!(cmd.transition, Transition::Ps);
    assert_eq!(cmd.index, 0x1C12);
    assert_eq!(cmd.sub_index, 0);
    assert_eq!(cmd.data, vec![0x00]);
    assert_eq!(cmd.comment.as_deref(), Some("clear RxPDO assign"));
}
