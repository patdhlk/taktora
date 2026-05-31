//! `netcfg` — command-line front end for the `ethercat-netcfg` toolchain.
//!
//! A thin clap shell over [`ethercat_netcfg_cli`]. The real work lives in
//! the library so it stays unit-testable; `main` only parses arguments,
//! prints the result, and maps errors to a non-zero exit code.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ethercat_netcfg_cli::{run_expand, run_fetch, run_verify};

/// `EtherCAT` network-config toolchain CLI.
#[derive(Debug, Parser)]
#[command(name = "netcfg", about, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate the Rust network module from a network.yaml and print it
    /// to stdout (for inspection / diffing).
    Expand {
        /// Path to the network.yaml to expand.
        yaml: PathBuf,
    },
    /// Vendor the ESI files referenced by a network.yaml into a local
    /// directory and pin each by SHA-256 in a JSON lockfile.
    Fetch {
        /// Path to the network.yaml to fetch ESI files for.
        yaml: PathBuf,
        /// Directory to vendor ESI files into (default: `vendor/esi`
        /// next to the network.yaml).
        #[arg(long)]
        vendor_dir: Option<PathBuf>,
        /// Lockfile path (default: `network.lock` next to the
        /// network.yaml).
        #[arg(long)]
        lockfile: Option<PathBuf>,
    },
    /// Verify that the ESI files referenced by a network.yaml still match
    /// their pins in the lockfile, failing loudly on any drift.
    Verify {
        /// Path to the network.yaml to verify.
        yaml: PathBuf,
        /// Lockfile path (default: `network.lock` next to the
        /// network.yaml).
        #[arg(long)]
        lockfile: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Expand { yaml } => match run_expand(&yaml) {
            Ok(source) => {
                print!("{source}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        },
        Commands::Fetch {
            yaml,
            vendor_dir,
            lockfile,
        } => {
            let parent = yaml.parent().unwrap_or_else(|| Path::new("."));
            let vendor_dir = vendor_dir.unwrap_or_else(|| parent.join("vendor").join("esi"));
            let lockfile = lockfile.unwrap_or_else(|| parent.join("network.lock"));
            match run_fetch(&yaml, &vendor_dir, &lockfile) {
                Ok(lock) => {
                    println!("pinned {} ESI file(s)", lock.entries.len());
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("error: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Commands::Verify { yaml, lockfile } => {
            let parent = yaml.parent().unwrap_or_else(|| Path::new("."));
            let lockfile = lockfile.unwrap_or_else(|| parent.join("network.lock"));
            match run_verify(&yaml, &lockfile) {
                Ok(()) => {
                    println!("verify: all ESI pins match");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("error: {err}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
