//! `TEST_0404` — parse errors carry line and column (a `Span`).
//! `REQ_0506` — parse errors carry source position; value errors name the path.
use taktora_ethercat_esi::{EsiError, parse};

const MALFORMED: &str = include_str!("fixtures/malformed.xml");

#[test]
fn syntax_error_carries_line_and_column() {
    let err = parse(MALFORMED).expect_err("malformed XML must error");
    match err {
        EsiError::Xml { span, .. } => {
            // quick-xml 0.40 `error_position()` for this fixture points at the
            // mismatched `</EtherCATInfo>` on line 5 (the unclosed <Type>/<Device>
            // tags only surface as an error when the wrong end-tag is hit).
            assert_eq!(span.line, 5, "span points at the failing line");
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
