use taktora_log_dlt::ids::CtxId;
use taktora_log_dlt::level_table::LevelTable;

#[test]
fn default_level_is_info() {
    let table = LevelTable::new(log::Level::Info);
    let ctx = CtxId::new("MAIN").unwrap();
    assert_eq!(table.current(&ctx), log::Level::Info);
}

#[test]
fn set_then_current_round_trips() {
    let table = LevelTable::new(log::Level::Info);
    let ctx = CtxId::new("MAIN").unwrap();
    table.set(&ctx, log::Level::Debug);
    assert_eq!(table.current(&ctx), log::Level::Debug);
}

#[test]
fn set_default_affects_unknown_contexts() {
    let table = LevelTable::new(log::Level::Info);
    table.set_default(log::Level::Warn);
    let unknown = CtxId::new("ANEW").unwrap();
    assert_eq!(table.current(&unknown), log::Level::Warn);
}
