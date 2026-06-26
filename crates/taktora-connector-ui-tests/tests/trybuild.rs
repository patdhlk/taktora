//! Compile-fail tests: rejected (non-POD) ViewModel field types must produce a
//! clear `compile_error!` (REQ_0858).

#[test]
fn rejected_field_types_fail_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/reject_vec.rs");
    t.compile_fail("tests/ui/reject_nested_struct.rs");
    t.compile_fail("tests/ui/reject_serde_rename_container.rs");
    t.compile_fail("tests/ui/reject_serde_rename_field.rs");
    t.compile_fail("tests/ui/reject_generic.rs");
}
