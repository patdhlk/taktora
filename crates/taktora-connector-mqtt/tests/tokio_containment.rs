//! `REQ_0258` (static piece) — verify the `taktora-connector-mqtt` public
//! API does NOT name any `tokio::` type in its surface. The tokio sidecar
//! ([`taktora_connector_mqtt::MqttGateway`]) is contained inside the crate;
//! no `tokio::` type leaks into taktora-executor's `WaitSet` thread.
//!
//! The runtime piece — asserting no tokio task handle attributable to the
//! MQTT sidecar appears in the executor's task list — is deferred to a
//! future stage that lands the necessary executor introspection (mirrors the
//! Zenoh crate's `TODO(Z6)`).

use std::process::Command;

#[test]
#[ignore = "requires cargo-public-api; run from the public-api-tests CI job"]
fn public_api_does_not_name_tokio_types() {
    let output = Command::new("cargo")
        .args([
            "public-api",
            "--manifest-path",
            "crates/taktora-connector-mqtt/Cargo.toml",
            "--simplified",
        ])
        .current_dir(workspace_root())
        .output()
        .expect(
            "cargo public-api invocation failed; install with \
             `cargo install cargo-public-api --locked`",
        );
    assert!(
        output.status.success(),
        "cargo public-api exited non-zero:\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let api = String::from_utf8_lossy(&output.stdout);
    let leaks: Vec<&str> = api.lines().filter(|l| l.contains("tokio::")).collect();
    assert!(
        leaks.is_empty(),
        "REQ_0258 violated: public API names tokio types:\n{}",
        leaks.join("\n")
    );
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}
