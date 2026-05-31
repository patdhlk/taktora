//! Integration tests for [`taktora_fieldbus_od_core::Identity`].

use taktora_fieldbus_od_core::Identity;

#[test]
fn identity_holds_the_vendor_product_revision_triple() {
    let id = Identity {
        vendor_id: 0x0000_0002,
        product_code: 0x0750_0354,
        revision: 0x0000_0001,
    };
    assert_eq!(id.vendor_id, 0x0000_0002);
    assert_eq!(id.product_code, 0x0750_0354);
    assert_eq!(id.revision, 1);
    assert_eq!(id.clone(), id);
}
