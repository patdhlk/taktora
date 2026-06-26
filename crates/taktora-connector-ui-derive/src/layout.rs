//! Field-type lowering shared by the `ViewModel` and `CommandParams` derives.
//!
//! Maps each authored Rust field type onto the closed POD `FieldType` set,
//! its `#[repr(C)]` image representation, and a conservative JSON-size budget.
//! Stub until Task 2.3.
