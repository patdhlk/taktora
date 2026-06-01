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

/// Write raw `bytes` to `<manifest>/esi/<name>`, creating the `esi` dir.
///
/// Used to lay down a file with a specific (non-UTF-8) byte encoding so the
/// build helper's decode path is exercised, not the test's UTF-8 assumptions.
fn write_esi_bytes(manifest: &Path, name: &str, bytes: &[u8]) {
    let esi_dir = manifest.join("esi");
    fs::create_dir_all(&esi_dir).expect("create esi dir");
    fs::write(esi_dir.join(name), bytes).expect("write esi bytes");
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
fn run_merges_and_sorts_multiple_files() {
    let manifest = tempfile::tempdir().expect("manifest tempdir");
    let out = tempfile::tempdir().expect("out tempdir");

    // Two single-device files derived from the EL3001-like fixture, each with a
    // distinct product code and type name so they yield distinct device
    // structs. File names sort non-trivially: `a_device.xml` must come before
    // `b_device.xml` regardless of glob iteration order.
    let dev_a = EL3001_LIKE
        .replace("#x0bb93052", "#x0aaa0001")
        .replace("EL3001-like", "DeviceAlpha");
    let dev_b = EL3001_LIKE
        .replace("#x0bb93052", "#x0bbb0002")
        .replace("EL3001-like", "DeviceBeta");
    // Write in reverse-sorted order to prove `run` sorts the inputs itself.
    write_esi(manifest.path(), "b_device.xml", &dev_b);
    write_esi(manifest.path(), "a_device.xml", &dev_a);

    let inputs = Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .run(manifest.path(), out.path())
        .expect("run should succeed");

    // Both files are returned, in sorted order (`a_*` before `b_*`).
    let expected_a = manifest.path().join("esi").join("a_device.xml");
    let expected_b = manifest.path().join("esi").join("b_device.xml");
    assert_eq!(
        inputs,
        vec![expected_a, expected_b],
        "inputs must be both files in sorted order",
    );

    // The generated module carries a device struct merged from each file.
    let generated = out.path().join("devices.rs");
    let source = fs::read_to_string(&generated).expect("read generated");
    syn::parse_str::<syn::File>(&source).expect("merged module must be valid Rust");
    assert!(
        source.contains("pub struct DeviceAlpha"),
        "missing first device struct in:\n{source}",
    );
    assert!(
        source.contains("pub struct DeviceBeta"),
        "missing second device struct in:\n{source}",
    );
}

/// An EL3001-like device whose `<Name>` carries a non-ASCII character (`±`).
/// `{ENC}` is a placeholder for the encoding declaration, and `{PM}` for the
/// raw byte(s) encoding `±` in that encoding. The device `Type` content
/// (`EL2004`) and `ProductCode` drive the generated ident, which stays ASCII.
const PM_NAME_TEMPLATE: &str = r##"<?xml version="1.0" encoding="{ENC}"?>
<EtherCATInfo>
  <Vendor>
    <Id>#x00000002</Id>
    <Name>Synthetic Vendor</Name>
  </Vendor>
  <Descriptions>
    <Devices>
      <Device Physics="YY">
        <Type ProductCode="#x07d43052" RevisionNo="#x00100000">EL2004</Type>
        <Name>EL2004 4Ch. Dig. Output {PM}10</Name>
        <GroupType>DigOut</GroupType>
        <Sm StartAddress="#x1000" ControlByte="#x26" Enable="1">MBoxOut</Sm>
        <Sm StartAddress="#x1080" ControlByte="#x22" Enable="1">MBoxIn</Sm>
        <Sm StartAddress="#x1100" ControlByte="#x00" Enable="1">Outputs</Sm>
        <Mailbox>
          <CoE SdoInfo="1" PdoAssign="0" PdoConfig="0" CompleteAccess="0"/>
        </Mailbox>
        <RxPdo Sm="2" Fixed="1" Mandatory="1">
          <Index>#x1600</Index>
          <Name>Channel 1</Name>
          <Entry>
            <Index>#x7000</Index>
            <SubIndex>1</SubIndex>
            <BitLen>1</BitLen>
            <Name>Output</Name>
            <DataType>BOOL</DataType>
          </Entry>
        </RxPdo>
      </Device>
    </Devices>
  </Descriptions>
</EtherCATInfo>
"##;

/// Real Beckhoff ESI files declare `encoding="ISO-8859-1"` and store high bytes
/// (e.g. `±` = `0xB1`) as single Latin-1 bytes, not UTF-8. The build helper must
/// honour the declared encoding when decoding, so a Latin-1 file is ingested and
/// generates a well-formed (ASCII-ident) device.
#[test]
fn run_ingests_iso_8859_1_encoded_file() {
    let manifest = tempfile::tempdir().expect("manifest tempdir");
    let out = tempfile::tempdir().expect("out tempdir");

    // Build the document as ISO-8859-1 bytes: ASCII text plus a literal 0xB1
    // for `±`. Splicing at the byte level guarantees the file is NOT valid
    // UTF-8 (0xB1 alone is an invalid UTF-8 continuation byte).
    let text = PM_NAME_TEMPLATE
        .replace("{ENC}", "ISO-8859-1")
        .replace("{PM}", "\u{0001}"); // sentinel we replace byte-wise below
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    for b in text.bytes() {
        if b == 0x01 {
            bytes.push(0xB1); // `±` in ISO-8859-1
        } else {
            bytes.push(b);
        }
    }
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "fixture must be invalid UTF-8 to truly exercise the decode path",
    );
    write_esi_bytes(manifest.path(), "el2004.xml", &bytes);

    let inputs = Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .run(manifest.path(), out.path())
        .expect("ISO-8859-1 file should be ingested");

    let expected_input = manifest.path().join("esi").join("el2004.xml");
    assert!(inputs.contains(&expected_input), "input should be listed");

    let generated = out.path().join("devices.rs");
    let source = fs::read_to_string(&generated).expect("read generated");
    syn::parse_str::<syn::File>(&source).expect("generated source must be valid Rust");
    assert!(
        source.contains("pub struct EL2004"),
        "missing device struct in:\n{source}",
    );
}

/// The same document saved as UTF-8 (with `±` as the two-byte UTF-8 sequence)
/// must still parse — regression guard for the default / UTF-8 decode path.
#[test]
fn run_ingests_utf8_encoded_file_with_non_ascii() {
    let manifest = tempfile::tempdir().expect("manifest tempdir");
    let out = tempfile::tempdir().expect("out tempdir");

    let text = PM_NAME_TEMPLATE
        .replace("{ENC}", "UTF-8")
        .replace("{PM}", "\u{00B1}"); // `±` as a proper UTF-8 char
    write_esi(manifest.path(), "el2004.xml", &text);

    let _ = Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .run(manifest.path(), out.path())
        .expect("UTF-8 file should be ingested");

    let generated = out.path().join("devices.rs");
    let source = fs::read_to_string(&generated).expect("read generated");
    syn::parse_str::<syn::File>(&source).expect("generated source must be valid Rust");
    assert!(
        source.contains("pub struct EL2004"),
        "missing device struct in:\n{source}",
    );
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
