use taktora_log_dlt::ids::{AppId, CtxId, IdError};

#[test]
fn appid_accepts_four_ascii() {
    let id = AppId::new("TKEX").expect("four-char ASCII");
    assert_eq!(id.as_str(), "TKEX");
}

#[test]
fn appid_rejects_wrong_length() {
    assert!(matches!(
        AppId::new("TKX"),
        Err(IdError::WrongLength { len: 3 })
    ));
    assert!(matches!(
        AppId::new("TKXYZ"),
        Err(IdError::WrongLength { len: 5 })
    ));
}

#[test]
fn appid_rejects_non_ascii() {
    // "TKÄ" is 4 bytes: T(1) + K(1) + Ä(U+00C4 = 2 bytes UTF-8).
    // Byte-length check passes (4 == 4); is_ascii() returns false → NonAscii.
    // Note: the plan literal used "TKÄX" (5 bytes), which would have triggered
    // WrongLength instead — corrected here per task instructions.
    assert!(matches!(AppId::new("TKÄ"), Err(IdError::NonAscii)));
}

#[test]
fn ctxid_same_rules() {
    assert!(CtxId::new("MAIN").is_ok());
    assert!(matches!(
        CtxId::new("MAI"),
        Err(IdError::WrongLength { len: 3 })
    ));
}
