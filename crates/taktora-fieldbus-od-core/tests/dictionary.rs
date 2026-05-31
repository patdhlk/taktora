use taktora_fieldbus_od_core::{Access, DataType, DictEntry};

#[test]
fn dict_entry_carries_index_subindex_type_and_access() {
    let entry = DictEntry {
        index: 0x6000,
        sub_index: 1,
        name: "Input".to_owned(),
        data_type: DataType::Bool,
        bit_size: Some(1),
        access: Access {
            read: true,
            write: false,
            pdo_mappable: true,
        },
        default: None,
    };
    assert_eq!(entry.index, 0x6000);
    assert_eq!(entry.sub_index, 1);
    assert!(entry.access.read);
    assert!(!entry.access.write);
    assert!(entry.access.pdo_mappable);
    assert_eq!(entry.default, None);
}

#[test]
fn dict_entry_default_is_raw_text() {
    let entry = DictEntry {
        index: 0x8000,
        sub_index: 0,
        name: "Param".to_owned(),
        data_type: DataType::U16,
        bit_size: Some(16),
        access: Access {
            read: true,
            write: true,
            pdo_mappable: false,
        },
        default: Some("#x0010".to_owned()),
    };
    assert_eq!(entry.default.as_deref(), Some("#x0010"));
}
