//! Testable core for the `ethercat-netcfg` CLI front end.
//!
//! The CLI binary (`netcfg`) is a thin clap shell over the functions in
//! this library. Keeping the work here — and returning `String`s rather
//! than printing — keeps the behaviour unit-testable without driving the
//! binary end-to-end.

#![warn(missing_docs)]

use std::path::Path;

/// Errors surfaced by the CLI core.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Reading the network.yaml from disk failed.
    #[error("failed to read network config: {0}")]
    Io(#[from] std::io::Error),
    /// Parsing the network.yaml into the IR failed.
    #[error("failed to parse network config: {0}")]
    Parse(#[from] ethercat_netcfg::NetcfgError),
    /// Generating Rust source from the IR failed.
    #[error("failed to generate network module: {0}")]
    Codegen(#[from] ethercat_netcfg_codegen::CodegenError),
}

/// Read the network config at `yaml_path`, parse it, and return the
/// generated Rust module as a source string.
///
/// This does *not* print — callers (the `netcfg expand` subcommand)
/// decide where the source goes.
pub fn run_expand(yaml_path: &Path) -> Result<String, CliError> {
    let yaml = std::fs::read_to_string(yaml_path)?;
    let config = ethercat_netcfg::parse(&yaml)?;
    let source = ethercat_netcfg_codegen::generate(&config)?;
    Ok(source)
}
