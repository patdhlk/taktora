//! `TEST_0859` — per-SM watchdog-trigger enable decodes from control-byte bit 6.
//! `REQ_0843` — the per-SM watchdog-trigger enable bit (control byte bit 6,
//! `0x40`) is decoded into [`SyncManager::watchdog_trigger_enable`].
//!
//! Each test builds a minimal valid ESI wrapper inline. The control-byte
//! values mirror real WAGO 750-354 sync managers: `#x64` is the outputs SM
//! (bit 6 set → watchdog trigger enabled), `#x22` the mailbox-in SM and `#x00`
//! the inputs SM (bit 6 clear → disabled). `#x24` is an additional clear case;
//! an `<Sm>` with no `ControlByte` attribute must default to `false` — the
//! parser never fabricates an enabled watchdog.
//!
//! Hex `#x` literals force `r##"…"##` raw-string delimiters here.
use taktora_ethercat_esi::parse;

/// Build a single-device ESI document with the given `<Sm>` control byte
/// (verbatim attribute text) and return that device's first sync manager's
/// decoded `watchdog_trigger_enable` flag.
fn watchdog_enable_for(control_byte_attr: &str) -> bool {
    let xml = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<EtherCATInfo>
  <Vendor><Id>2</Id><Name>V</Name></Vendor>
  <Descriptions><Devices>
    <Device>
      <Type ProductCode="256" RevisionNo="1">A</Type>
      <Sm StartAddress="#x1000"{control_byte_attr} Enable="1">SM</Sm>
    </Device>
  </Devices></Descriptions>
</EtherCATInfo>"##
    );
    let file = parse(&xml).expect("fixture parses");
    file.devices[0].sync_managers[0].watchdog_trigger_enable
}

/// `#x64` = `0b0110_0100`: bit 6 set → watchdog trigger enabled. This is the
/// real WAGO 750-354 outputs sync manager.
#[test]
fn control_byte_0x64_enables_watchdog_trigger() {
    assert!(watchdog_enable_for(r##" ControlByte="#x64""##));
}

/// `#x22` = `0b0010_0010`: bit 6 clear → disabled. WAGO mailbox-in SM.
#[test]
fn control_byte_0x22_leaves_watchdog_trigger_disabled() {
    assert!(!watchdog_enable_for(r##" ControlByte="#x22""##));
}

/// `#x00`: all bits clear → disabled. WAGO inputs SM.
#[test]
fn control_byte_0x00_leaves_watchdog_trigger_disabled() {
    assert!(!watchdog_enable_for(r##" ControlByte="#x00""##));
}

/// `#x24` = `0b0010_0100`: bit 6 clear → disabled.
#[test]
fn control_byte_0x24_leaves_watchdog_trigger_disabled() {
    assert!(!watchdog_enable_for(r##" ControlByte="#x24""##));
}

/// An `<Sm>` with no `ControlByte` attribute must default to `false`: an absent
/// control byte never fabricates an enabled watchdog.
#[test]
fn missing_control_byte_defaults_watchdog_trigger_to_false() {
    assert!(!watchdog_enable_for(""));
}
