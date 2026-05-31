//! `REQ_0506` — parse errors carry source position; value errors name the path.
use taktora_ethercat_esi::{EsiError, parse};

const MALFORMED: &str = include_str!("fixtures/malformed.xml");

#[test]
fn syntax_error_carries_line_and_column() {
    let err = parse(MALFORMED).expect_err("malformed XML must error");
    match err {
        EsiError::Xml { span, .. } => {
            assert_eq!(span.line, 4, "span points at the failing line");
        }
        other => panic!("expected Xml error, got {other:?}"),
    }
}

#[test]
fn bad_integer_value_names_the_path() {
    let xml = r##"<?xml version="1.0"?>
<EtherCATInfo><Vendor><Id>#x2</Id></Vendor><Descriptions><Devices>
<Device><Type ProductCode="NOTANUMBER" RevisionNo="#x1">X</Type></Device>
</Devices></Descriptions></EtherCATInfo>"##;
    let err = parse(xml).expect_err("bad ProductCode must error");
    match err {
        EsiError::Number { path, raw } => {
            assert_eq!(raw, "NOTANUMBER");
            assert!(path.contains("ProductCode"), "path names the field: {path}");
        }
        other => panic!("expected Number error, got {other:?}"),
    }
}
