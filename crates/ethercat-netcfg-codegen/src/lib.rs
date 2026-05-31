//! Code generator for the `ethercat-netcfg` IR.
//!
//! Turns a parsed [`ethercat_netcfg::NetworkConfig`] into a `String` of
//! formatted Rust source for the `taktora-connector-ethercat` runtime.
//! The runtime types are named **textually** so this crate never
//! depends on `taktora-connector-ethercat`.

#![warn(missing_docs)]

use ethercat_netcfg::{DeviceSource, NetworkConfig, PdoEntry};
use proc_macro2::TokenStream;
use quote::quote;

/// Errors that can occur while [`generate`]-ing Rust source.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// The internally generated token stream failed to parse back into a
    /// `syn::File`. This indicates a bug in the generator, not in the
    /// input config.
    #[error("generated token stream is not a valid Rust file: {0}")]
    Syn(#[from] syn::Error),
}

/// Generate formatted Rust source for the given network config.
///
/// This slice emits a single item: a `pub static PDO_MAP` of
/// `taktora_connector_ethercat::SubDeviceMap` entries, one per device.
pub fn generate(config: &NetworkConfig) -> Result<String, CodegenError> {
    let tokens = pdo_map_tokens(config);
    let file: syn::File = syn::parse2(tokens)?;
    Ok(prettyplease::unparse(&file))
}

/// Build the `static PDO_MAP` token stream.
fn pdo_map_tokens(config: &NetworkConfig) -> TokenStream {
    let entries = config.devices.iter().map(|device| {
        // Slice 3 will derive the real configured address; for now emit
        // a placeholder.
        let address: u16 = 0x1000;
        // Slice 5 will derive `expected_wkc`; placeholder for now.
        let expected_wkc: u16 = 0;

        let DeviceSource::Inline { rx, tx } = &device.source;
        let rx_entries = rx.iter().map(pdo_entry_tokens);
        let tx_entries = tx.iter().map(pdo_entry_tokens);

        quote! {
            taktora_connector_ethercat::SubDeviceMap {
                address: #address,
                rx_pdos: &[ #(#rx_entries),* ],
                tx_pdos: &[ #(#tx_entries),* ],
                expected_wkc: #expected_wkc,
            }
        }
    });

    quote! {
        pub static PDO_MAP: &[taktora_connector_ethercat::SubDeviceMap] = &[
            #(#entries),*
        ];
    }
}

/// Emit one `taktora_connector_ethercat::PdoEntry` literal.
fn pdo_entry_tokens(entry: &PdoEntry) -> TokenStream {
    let index = entry.index;
    let bit_offset = entry.bit_offset;
    let bit_length = entry.bit_length;
    quote! {
        taktora_connector_ethercat::PdoEntry {
            index: #index,
            bit_offset: #bit_offset,
            bit_length: #bit_length,
        }
    }
}
