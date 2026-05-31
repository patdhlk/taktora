//! Testable core for the `ethercat-netcfg` CLI front end.
//!
//! The CLI binary (`netcfg`) is a thin clap shell over the functions in
//! this library. Keeping the work here — and returning `String`s rather
//! than printing — keeps the behaviour unit-testable without driving the
//! binary end-to-end.

#![warn(missing_docs)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    /// Serializing the lockfile to JSON failed.
    #[error("failed to serialize lockfile: {0}")]
    Json(#[from] serde_json::Error),
    /// A referenced ESI device has no matching lockfile entry.
    #[error("ESI reference is not pinned in the lockfile: {reference}")]
    Unpinned {
        /// The unpinned ESI reference.
        reference: String,
    },
    /// A referenced ESI file's content hash no longer matches its pin.
    #[error("ESI content hash drift for {reference}: pinned {expected}, found {actual}")]
    HashMismatch {
        /// The drifting ESI reference.
        reference: String,
        /// The SHA-256 hex pinned in the lockfile.
        expected: String,
        /// The SHA-256 hex of the current file content.
        actual: String,
    },
    /// A referenced ESI device's revision no longer matches its pin.
    #[error("ESI revision drift for {reference}: pinned {expected}, found {actual}")]
    RevisionMismatch {
        /// The drifting ESI reference.
        reference: String,
        /// The revision pinned in the lockfile.
        expected: u32,
        /// The revision resolved from the current ESI file.
        actual: u32,
    },
}

/// Compute the lowercase-hex SHA-256 over `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

/// A vendored-and-pinned ESI lockfile: the result of `netcfg fetch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    /// One entry per ESI-sourced device, in declaration order.
    pub entries: Vec<LockEntry>,
}

/// A single pinned ESI file within a [`Lockfile`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    /// The ESI reference as written in the network config.
    pub reference: String,
    /// Path to the vendored copy of the ESI file.
    pub vendored: PathBuf,
    /// Lowercase-hex SHA-256 over the original ESI file bytes.
    pub sha256: String,
    /// Device revision, resolved from the ESI file at parse time.
    pub revision: u32,
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

/// Vendor every ESI file referenced by the config at `yaml_path` into
/// `vendor_dir`, pinning each by SHA-256, and write the resulting
/// [`Lockfile`] to `lockfile_path` as pretty JSON.
///
/// Inline-only configs yield an empty lockfile and no vendored files.
pub fn run_fetch(
    yaml_path: &Path,
    vendor_dir: &Path,
    lockfile_path: &Path,
) -> Result<Lockfile, CliError> {
    let yaml = std::fs::read_to_string(yaml_path)?;
    let config = ethercat_netcfg::parse(&yaml)?;

    let mut entries = Vec::new();
    for device in &config.devices {
        let ethercat_netcfg::DeviceSource::Esi { path, .. } = &device.source else {
            continue;
        };

        let bytes = std::fs::read(path)?;
        let hex = sha256_hex(&bytes);

        std::fs::create_dir_all(vendor_dir)?;
        let file_name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("esi.xml"));
        let vendored = vendor_dir.join(file_name);
        std::fs::write(&vendored, &bytes)?;

        let revision = device.identity.as_ref().map_or(0, |id| id.revision);

        entries.push(LockEntry {
            reference: path.display().to_string(),
            vendored,
            sha256: hex,
            revision,
        });
    }

    let lockfile = Lockfile { entries };
    let json = serde_json::to_string_pretty(&lockfile)?;
    std::fs::write(lockfile_path, json)?;
    Ok(lockfile)
}

/// Verify that every ESI-sourced device in the config at `yaml_path` still
/// matches its pin in the lockfile at `lockfile_path` (`REQ_0835`).
///
/// For each `DeviceSource::Esi` device this checks that a matching
/// [`LockEntry`] exists, that the file's current SHA-256 matches the pin,
/// and that the device's resolved revision matches the pinned revision.
/// Any drift is a loud error.
pub fn run_verify(yaml_path: &Path, lockfile_path: &Path) -> Result<(), CliError> {
    let yaml = std::fs::read_to_string(yaml_path)?;
    let config = ethercat_netcfg::parse(&yaml)?;

    let lockfile_json = std::fs::read_to_string(lockfile_path)?;
    let lockfile: Lockfile = serde_json::from_str(&lockfile_json)?;

    for device in &config.devices {
        let ethercat_netcfg::DeviceSource::Esi { path, .. } = &device.source else {
            continue;
        };
        let reference = path.display().to_string();

        let Some(entry) = lockfile.entries.iter().find(|e| e.reference == reference) else {
            return Err(CliError::Unpinned { reference });
        };

        let bytes = std::fs::read(path)?;
        let actual = sha256_hex(&bytes);
        if actual != entry.sha256 {
            return Err(CliError::HashMismatch {
                reference,
                expected: entry.sha256.clone(),
                actual,
            });
        }

        let revision = device.identity.as_ref().map_or(0, |id| id.revision);
        if revision != entry.revision {
            return Err(CliError::RevisionMismatch {
                reference,
                expected: entry.revision,
                actual: revision,
            });
        }
    }

    Ok(())
}
