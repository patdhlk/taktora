//! REQ_0809: log::kv pairs encoded as DLT verbose arguments with
//! native types where possible.

use taktora_log_dlt::encode::Encoder;
use taktora_log_dlt::ids::{AppId, CtxId};

#[test]
fn kv_types_map_to_dlt_value_variants() {
    let encoder = Encoder::new(
        AppId::new("TKEX").unwrap(),
        CtxId::new("MAIN").unwrap(),
        "ECU1".to_string(),
    );

    use log::kv::{Key, Source, Value};

    struct Pairs;
    impl Source for Pairs {
        fn visit<'kvs>(
            &'kvs self,
            visitor: &mut dyn log::kv::VisitSource<'kvs>,
        ) -> Result<(), log::kv::Error> {
            visitor.visit_pair(Key::from("count"), Value::from(7u32))?;
            visitor.visit_pair(Key::from("score"), Value::from(1.5f64))?;
            visitor.visit_pair(Key::from("ok"), Value::from(true))?;
            visitor.visit_pair(Key::from("name"), Value::from("alice"))?;
            Ok(())
        }
    }

    let args = format_args!("hi");
    let rec = log::Record::builder()
        .level(log::Level::Info)
        .target("tk.test")
        .args(args)
        .key_values(&Pairs)
        .build();

    let bytes = encoder.encode(&rec, 0);
    // dlt_message returns (remaining: &[u8], ParsedMessage)
    let (_remaining, parsed) = dlt_core::parse::dlt_message(&bytes, None, true).unwrap();
    let msg = match parsed {
        dlt_core::parse::ParsedMessage::Item(m) => m,
        other => panic!("expected ParsedMessage::Item, got {other:?}"),
    };
    if let dlt_core::dlt::PayloadContent::Verbose(args) = &msg.payload {
        // [0] = formatted message; [1..] = kv pairs in source order.
        assert!(
            matches!(args[1].value, dlt_core::dlt::Value::U32(7)),
            "expected U32(7), got {:?}",
            args[1].value
        );
        assert!(
            matches!(args[2].value, dlt_core::dlt::Value::F64(v) if (v - 1.5).abs() < 1e-9),
            "expected F64(~1.5), got {:?}",
            args[2].value
        );
        assert!(
            matches!(args[3].value, dlt_core::dlt::Value::Bool(1)),
            "expected Bool(1), got {:?}",
            args[3].value
        );
        assert!(
            matches!(&args[4].value, dlt_core::dlt::Value::StringVal(s) if s == "alice"),
            "expected StringVal(\"alice\"), got {:?}",
            args[4].value
        );
    } else {
        panic!("payload not verbose");
    }
}
