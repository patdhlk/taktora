//! REQ_0806: encoder produces parseable DLT R20-11 bytes via dlt-core.
//!
//! Round-trip test: encode a synthetic `log::Record`, parse the bytes
//! back with `dlt-core`, assert that App ID, Context ID, level, and
//! message body survive.

use taktora_log_dlt::encode::Encoder;
use taktora_log_dlt::ids::{AppId, CtxId};

#[test]
fn encode_then_parse_recovers_fields() {
    let encoder = Encoder::new(
        AppId::new("TKEX").unwrap(),
        CtxId::new("MAIN").unwrap(),
        "ECU1".to_string(),
    );
    let args = format_args!("hello world {}", 7);
    let rec = log::Record::builder()
        .level(log::Level::Info)
        .target("tk.test")
        .args(args)
        .build();

    let bytes = encoder.encode(&rec, /*timestamp_tenths_ms=*/ 1234);
    assert!(!bytes.is_empty(), "encoder produced no bytes");

    // Parse with dlt-core and check the recovered message.
    // dlt-core returns `(remaining, ParsedMessage)` where `remaining` is
    // the unconsumed tail of `input` (nom convention).
    let (remaining, parsed) =
        dlt_core::parse::dlt_message(&bytes, None, true).expect("dlt-core parses our output");
    assert!(
        remaining.is_empty(),
        "trailing bytes left after parse: {} byte(s)",
        remaining.len()
    );
    let msg = match parsed {
        dlt_core::parse::ParsedMessage::Item(m) => m,
        other => panic!("expected ParsedMessage::Item, got {:?}", other),
    };
    let ext = msg
        .extended_header
        .as_ref()
        .expect("verbose message has extended header");
    assert_eq!(ext.application_id, "TKEX");
    assert_eq!(ext.context_id, "MAIN");
    assert!(matches!(
        ext.message_type,
        dlt_core::dlt::MessageType::Log(dlt_core::dlt::LogLevel::Info)
    ));

    // Payload contains the formatted message as the first string arg.
    if let dlt_core::dlt::PayloadContent::Verbose(args) = &msg.payload {
        assert!(!args.is_empty(), "no arguments in payload");
        if let dlt_core::dlt::Value::StringVal(s) = &args[0].value {
            assert_eq!(s, "hello world 7");
        } else {
            panic!("first arg is not a string: {:?}", args[0].value);
        }
    } else {
        panic!("payload is not verbose");
    }
}
