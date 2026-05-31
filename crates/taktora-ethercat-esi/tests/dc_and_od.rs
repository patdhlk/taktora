//! `REQ_0504` — distributed clocks + object dictionary in the IR.
use taktora_ethercat_esi::{Access, DataType, parse};

const EL3001: &str = include_str!("fixtures/el3001_like.xml");
const DICT: &str = include_str!("fixtures/dictionary.xml");

#[test]
fn parses_distributed_clock_op_modes() {
    let file = parse(EL3001).expect("fixture parses");
    let dc = file.devices[0].dc.as_ref().expect("device declares DC");
    assert_eq!(dc.op_modes.len(), 1);
    let m = &dc.op_modes[0];
    assert_eq!(m.name, "Sync");
    assert_eq!(m.desc.as_deref(), Some("DC-Synchron"));
    assert_eq!(m.assign_activate, 0x0300);
    assert_eq!(m.cycle_time_sync0, Some(0));
}

#[test]
fn parses_object_dictionary_entries() {
    let file = parse(DICT).expect("fixture parses");
    let od = &file.devices[0].dictionary;
    assert_eq!(od.len(), 1);
    let e = &od[0];
    assert_eq!(e.index, 0x6000);
    assert_eq!(e.sub_index, 1);
    assert_eq!(e.name, "Underrange");
    assert_eq!(e.data_type, DataType::Bool);
    assert_eq!(
        e.access,
        Access {
            read: true,
            write: false,
            pdo_mappable: true
        }
    );
    assert_eq!(e.default.as_deref(), Some("#x00"));
}
