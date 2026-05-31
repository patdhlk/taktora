//! `netcfg` — command-line front end for the `ethercat-netcfg` toolchain.
//!
//! A thin clap shell over [`ethercat_netcfg_cli`]. The real work lives in
//! the library so it stays unit-testable; `main` only parses arguments,
//! prints the result, and maps errors to a non-zero exit code.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ethercat_netcfg_cli::run_expand;

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
    // `Fetch` (vendor ESI resolution) slots in here — phase 4.
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
    }
}
