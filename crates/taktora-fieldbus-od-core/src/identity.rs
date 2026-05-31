//! The vendor/product/revision identity triple.

use serde::{Deserialize, Serialize};

/// The vendor / product / revision triple identifying a fieldbus device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identity {
    /// Fieldbus vendor id.
    pub vendor_id: u32,
    /// Device product code.
    pub product_code: u32,
    /// Device revision number.
    pub revision: u32,
}
