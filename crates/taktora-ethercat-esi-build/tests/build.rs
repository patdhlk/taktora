//! Integration tests for the ESI build-script helper.
//!
//! Tests drive the testable seam [`Builder::run`] directly with temp dirs so
//! they never mutate `OUT_DIR` / `CARGO_MANIFEST_DIR` (which is `unsafe` in
//! edition 2024 and forbidden by this crate's lint table).

use std::fs;
use std::path::Path;

use taktora_ethercat_esi_build::{BuildError, Builder};

const EL3001_LIKE: &str = include_str!("../../taktora-ethercat-esi/tests/fixtures/el3001_like.xml");

/// Write `contents` to `<manifest>/esi/<name>`, creating the `esi` dir.
fn write_esi(manifest: &Path, name: &str, contents: &str) {
    let esi_dir = manifest.join("esi");
    fs::create_dir_all(&esi_dir).expect("create esi dir");
    fs::write(esi_dir.join(name), contents).expect("write esi file");
}

#[test]
fn run_generates_valid_module_for_single_file() {
    let manifest = tempfile::tempdir().expect("manifest tempdir");
    let out = tempfile::tempdir().expect("out tempdir");
    write_esi(manifest.path(), "el3001_like.xml", EL3001_LIKE);

    let inputs = Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .run(manifest.path(), out.path())
        .expect("run should succeed");

    // The returned input list names the matched ESI file.
    let expected_input = manifest.path().join("esi").join("el3001_like.xml");
    assert!(
        inputs.contains(&expected_input),
        "returned inputs {inputs:?} should contain {expected_input:?}",
    );

    // The generated file exists and is valid Rust.
    let generated = out.path().join("devices.rs");
    assert!(generated.exists(), "generated file should exist");
    let source = fs::read_to_string(&generated).expect("read generated");
    syn::parse_str::<syn::File>(&source).expect("generated source must be valid Rust");

    // It carries the expected device struct and identity constant.
    assert!(
        source.contains("pub struct EL3001_like"),
        "missing device struct in:\n{source}",
    );
    assert!(
        source.contains("EL3001_LIKE_REV00100000"),
        "missing identity constant in:\n{source}",
    );
}

#[test]
fn run_succeeds_with_empty_glob() {
    let manifest = tempfile::tempdir().expect("manifest tempdir");
    let out = tempfile::tempdir().expect("out tempdir");
    // Create an empty esi dir so the glob matches nothing.
    fs::create_dir_all(manifest.path().join("esi")).expect("create esi dir");

    let inputs = Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .run(manifest.path(), out.path())
        .expect("empty glob should still succeed");

    assert!(inputs.is_empty(), "no inputs expected for empty glob");

    let generated = out.path().join("devices.rs");
    assert!(
        generated.exists(),
        "an empty module should still be written"
    );
    let source = fs::read_to_string(&generated).expect("read generated");
    syn::parse_str::<syn::File>(&source).expect("empty module must be valid Rust");
}

#[test]
fn run_reports_parse_error_with_file_path() {
    let manifest = tempfile::tempdir().expect("manifest tempdir");
    let out = tempfile::tempdir().expect("out tempdir");
    write_esi(manifest.path(), "broken.xml", "<EtherCATInfo><Vendor>");

    let err = Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .run(manifest.path(), out.path())
        .expect_err("malformed XML should error");

    assert!(matches!(err, BuildError::Parse { .. }));
    let rendered = err.to_string();
    assert!(
        rendered.contains("broken.xml"),
        "error {rendered:?} should name the offending file",
    );
}
