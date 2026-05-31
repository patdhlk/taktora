//! Build-glue for the `taktora-ethercat-netcfg` toolchain.
//!
//! A consumer's `build.rs` calls [`run`] to turn a `network.yaml` into a
//! generated Rust module in `OUT_DIR`. The unit-testable core is [`emit`];
//! [`run`] is a thin wrapper that reads `OUT_DIR` from the environment and
//! prints the `cargo:rerun-if-changed` directives.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

/// Errors that can occur while emitting the generated network module.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// Reading the YAML or writing the generated file failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Parsing the `network.yaml` failed.
    #[error(transparent)]
    Parse(#[from] taktora_ethercat_netcfg::NetcfgError),
    /// Generating Rust source from the parsed config failed.
    #[error(transparent)]
    Codegen(#[from] taktora_ethercat_netcfg_codegen::CodegenError),
}

/// Outcome of a successful [`emit`].
#[derive(Debug, Clone)]
pub struct EmitOutcome {
    /// Path of the written generated module (`<out_dir>/network.rs`).
    pub generated: PathBuf,
    /// Paths a build script should re-run on when they change.
    pub rerun_if_changed: Vec<PathBuf>,
}

/// Read `yaml_path`, parse + generate it, and write the result to
/// `<out_dir>/network.rs`.
pub fn emit(yaml_path: &Path, out_dir: &Path) -> Result<EmitOutcome, BuildError> {
    let yaml = std::fs::read_to_string(yaml_path)?;
    let config = taktora_ethercat_netcfg::parse(&yaml)?;
    let source = taktora_ethercat_netcfg_codegen::generate(&config)?;

    let generated = out_dir.join("network.rs");
    std::fs::write(&generated, source)?;

    Ok(EmitOutcome {
        generated,
        rerun_if_changed: vec![yaml_path.to_path_buf()],
    })
}

/// Call from a consumer `build.rs`: reads `OUT_DIR`, emits, and prints the
/// `cargo:rerun-if-changed` directives for each tracked path.
pub fn run(yaml_path: &Path) -> Result<(), BuildError> {
    let out_dir = std::env::var("OUT_DIR")
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "OUT_DIR not set"))?;
    let outcome = emit(yaml_path, Path::new(&out_dir))?;
    for path in &outcome.rerun_if_changed {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    Ok(())
}
