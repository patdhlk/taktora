//! REQ_0810: control messages from dlt-daemon update per-context levels.

use taktora_log_dlt::control::ControlMessage;
use taktora_log_dlt::ids::CtxId;
use taktora_log_dlt::level_table::LevelTable;

#[test]
fn set_log_level_applies_to_context() {
    let table = LevelTable::new(log::Level::Info);

    // Synthetic control payload — service 0x01 (Set-Log-Level),
    // CtxId "MAIN", level Debug (=5). We bypass parsing from DLT
    // bytes (which is dlt-core's job) and exercise the apply step.
    let msg = ControlMessage::SetLogLevel {
        ctx: CtxId::new("MAIN").unwrap(),
        level: log::Level::Debug,
    };
    msg.apply(&table);

    assert_eq!(
        table.current(&CtxId::new("MAIN").unwrap()),
        log::Level::Debug
    );
}

#[test]
fn set_default_applies_globally() {
    let table = LevelTable::new(log::Level::Info);
    let msg = ControlMessage::SetDefaultLogLevel(log::Level::Warn);
    msg.apply(&table);
    assert_eq!(
        table.current(&CtxId::new("NEW1").unwrap()),
        log::Level::Warn
    );
}
